mod commands;
mod core;
mod error;
mod models;

use commands::config::{
    get_config, get_last_selected_agents, save_config, save_last_selected_agents,
    add_project, remove_project, check_project_path, open_in_explorer,
};
use commands::agents::list_agents;
use commands::install::{fetch_available, install_skills};
use commands::overwrites::check_overwrites;
use commands::remove::remove_skill;
use commands::skills::list_skills;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_agents,
            list_skills,
            get_config,
            save_config,
            get_last_selected_agents,
            save_last_selected_agents,
            add_project,
            remove_project,
            check_project_path,
            open_in_explorer,
            fetch_available,
            install_skills,
            check_overwrites,
            remove_skill,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

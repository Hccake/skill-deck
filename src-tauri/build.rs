#[path = "app_commands.rs"]
mod app_commands;

fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(app_commands::APP_COMMANDS)),
    )
    .expect("failed to build Tauri application manifest");
}

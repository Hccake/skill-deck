#[macro_use]
#[path = "app_commands.rs"]
mod app_commands;

macro_rules! command_names {
    ($($name:ident => [$($handler:tt)+]),* $(,)?) => {
        &[$(stringify!($name)),*]
    };
}

const APP_COMMANDS: &[&str] = with_app_commands!(command_names);

fn main() {
    let windows_target = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows");
    let mut attributes = tauri_build::Attributes::new()
        .app_manifest(tauri_build::AppManifest::new().commands(APP_COMMANDS));

    if windows_target {
        attributes = attributes
            .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest());
    }

    tauri_build::try_build(attributes).expect("failed to build Tauri application manifest");

    if windows_target {
        embed_resource::compile_for_everything("windows-app-manifest.rc", embed_resource::NONE)
            .manifest_required()
            .expect("failed to embed the Windows application manifest");
    }
}

#[cfg(debug_assertions)]
use specta_typescript::{BigIntExportBehavior, Typescript};
use tauri::{Emitter, Manager};
use tauri_plugin_log::{Target, TargetKind};
use tauri_specta::{collect_commands, collect_events, Builder, Event};

use application::install_wizard_session::InstallWizardSessionSnapshot;
use commands::lifecycle::LifecycleActionRequestedEvent;
use commands::ManagedAgentRegistry;
use environment::types::EnvironmentRuntimeEvent;
use runtime::RuntimeServiceGraph;

mod application;
mod background_process;
mod commands;
mod core;
mod environment;
mod error;
mod models;
mod runtime;
mod storage;

#[cfg(test)]
#[path = "test_support/git_fixture.rs"]
mod git_fixture;
#[cfg(test)]
#[path = "test_support/native_workflow.rs"]
mod native_workflow_integration_support;

fn specta_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new()
        .commands(collect_commands![
            commands::agent_selection::get_install_agent_selection,
            commands::source_acquisition::acquire_selected_payloads,
            commands::agents::list_agents,
            commands::agents::get_agent_settings_snapshot,
            commands::agents::validate_custom_agent_draft,
            commands::agents::save_custom_agent,
            commands::agents::delete_custom_agent,
            commands::agents::delete_invalid_custom_agent,
            commands::agents::preview_custom_agent_delete,
            commands::skills::list_skills,
            commands::skills::read_skill_content,
            commands::config::get_config,
            commands::config::save_config,
            commands::config::set_wsl_integration_enabled,
            commands::github_credentials::get_github_credential_status,
            commands::github_credentials::save_github_credential,
            commands::github_credentials::clear_github_credential,
            commands::install::fetch_available,
            commands::install_workflow::preview_install,
            commands::install_workflow::install_skills,
            commands::lifecycle::execute_lifecycle_action,
            commands::remove::preview_remove,
            commands::remove::remove_skill,
            commands::recovery::list_recovery_resources,
            commands::recovery::get_recovery_resource_status,
            commands::recovery::confirm_recovery_resource_resolved,
            commands::recovery::open_recovery_resource,
            commands::resources::open_skill_resource,
            commands::resources::open_config_resource,
            commands::duplicate_copies::cleanup_duplicate_agent_copies,
            commands::update::check_updates,
            commands::update::preview_update,
            commands::update::update_skill,
            commands::update::update_skills_batch,
            commands::wizard::open_install_wizard,
            commands::wizard::get_install_wizard_session,
            commands::wizard::focus_install_wizard,
            commands::audit::check_skill_audit,
            commands::manage_agents::preview_manage_skill_agents,
            commands::manage_agents::get_manage_agent_selection,
            commands::manage_agents::manage_skill_agents,
            commands::copy_skill::get_copy_agent_selection,
            commands::copy_skill::preview_copy_skill_to_projects,
            commands::copy_skill::copy_skill_to_projects,
            commands::environments::list_environments,
            commands::environments::connect_environment,
            commands::environments::map_environment_path,
            commands::environments::list_environment_projects,
            commands::environments::add_environment_project,
            commands::environments::remove_environment_project,
            commands::environments::set_environment_project_cross_storage_warning,
            commands::environments::retry_host_project_migration,
            commands::mutations::get_active_mutation,
            commands::mutations::request_cancel_active_mutation,
            commands::updater::check_application_update,
            commands::updater::download_and_install_application_update,
        ])
        .events(collect_events![
            EnvironmentRuntimeEvent,
            InstallWizardSessionSnapshot,
            LifecycleActionRequestedEvent,
        ])
}

#[cfg(debug_assertions)]
fn bindings_output_path(override_path: Option<std::ffi::OsString>) -> std::path::PathBuf {
    override_path
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/bindings.ts")
        })
}

#[cfg(debug_assertions)]
fn normalize_generated_bindings(path: &std::path::Path) -> std::io::Result<()> {
    let content = std::fs::read_to_string(path)?;
    let mut normalized = content
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n");
    normalized.push('\n');
    std::fs::write(path, normalized)
}

#[cfg(debug_assertions)]
fn export_typescript_bindings(builder: &Builder<tauri::Wry>) {
    let path = bindings_output_path(std::env::var_os("SKILL_DECK_BINDINGS_OUT"));
    builder
        .export(
            Typescript::default()
                .bigint(BigIntExportBehavior::Number)
                .header("// 此文件由 tauri-specta 自动生成，请勿手动修改\n// This file is auto-generated by tauri-specta. Do not edit manually."),
            &path,
        )
        .expect("Failed to export typescript bindings");
    normalize_generated_bindings(&path).expect("Failed to normalize typescript bindings");
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = specta_builder();
    let agent_registry = ManagedAgentRegistry::for_current_user();

    #[cfg(debug_assertions)]
    export_typescript_bindings(&builder);

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_log::Builder::default()
                .level(log::LevelFilter::Info)
                .targets([
                    Target::new(TargetKind::Stdout),
                    Target::new(TargetKind::LogDir { file_name: None }),
                ])
                .build(),
        )
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            let payload_cache_root = app.path().app_cache_dir()?.join("payload-sessions");
            let recovery_root = app.path().app_local_data_dir()?.join("recovery");
            let runtime = RuntimeServiceGraph::new(
                &payload_cache_root,
                recovery_root,
                agent_registry.clone(),
            )?;
            let environments = runtime.wsl_arc();
            let maintenance = runtime.maintenance().clone();

            let environment_app_handle = app.handle().clone();
            let maintenance_for_environments = maintenance.clone();
            environments.set_listener(move |event| {
                if let Err(error) = event.clone().emit(&environment_app_handle) {
                    log::warn!("Failed to emit environment runtime state: {error}");
                }
                if event.status == environment::types::EnvironmentStatus::Available
                    && matches!(
                        event.environment,
                        environment::types::EnvironmentRef::Wsl { .. }
                    )
                {
                    let environment = event.environment;
                    let revision = event.revision;
                    if let Err(error) = maintenance_for_environments.register(environment.clone()) {
                        log::warn!("Failed to register WSL runtime maintenance: {error}");
                        return;
                    }
                    let maintenance = maintenance_for_environments.clone();
                    tauri::async_runtime::spawn(async move {
                        if let Err(error) = maintenance.start(environment, revision).await {
                            log::warn!("Failed to run WSL runtime maintenance: {error}");
                        }
                    });
                }
            });

            let mutation_app_handle = app.handle().clone();
            runtime.admission().set_mutation_listener(move |snapshot| {
                if let Err(error) =
                    mutation_app_handle.emit(core::mutation::MUTATION_STATE_CHANGED_EVENT, snapshot)
                {
                    log::warn!("Failed to emit mutation state: {error}");
                }
            });

            let wizard_session_app_handle = app.handle().clone();
            runtime
                .admission()
                .set_install_wizard_listener(move |snapshot| {
                    if let Err(error) = snapshot.emit_to(&wizard_session_app_handle, "main") {
                        log::warn!("Failed to emit install wizard session state: {error}");
                    }
                });

            let host_environment = environment::types::EnvironmentRef::Host;
            maintenance.register(host_environment.clone())?;
            let host_maintenance = maintenance.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(error) = host_maintenance.start(host_environment, 0).await {
                    log::warn!("Failed to run Host runtime maintenance: {error}");
                }
            });

            app.manage(runtime);
            builder.mount_events(app);

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod command_surface_tests {
    use std::collections::BTreeSet;

    fn registered_commands() -> BTreeSet<String> {
        let source = include_str!("lib.rs");
        source
            .split("collect_commands![")
            .nth(1)
            .and_then(|source| source.split("])").next())
            .expect("registered command list")
            .lines()
            .filter_map(|line| {
                line.trim()
                    .strip_suffix(',')
                    .and_then(|entry| entry.rsplit("::").next())
                    .filter(|entry| !entry.is_empty())
                    .map(str::to_string)
            })
            .collect()
    }

    fn app_manifest_commands() -> BTreeSet<String> {
        include_str!("../app_commands.rs")
            .lines()
            .filter_map(|line| {
                line.trim()
                    .strip_prefix('"')
                    .and_then(|line| line.strip_suffix("\","))
                    .map(str::to_string)
            })
            .collect()
    }

    #[test]
    fn bindings_output_path_prefers_explicit_override() {
        let override_path = std::ffi::OsString::from("/tmp/generated-bindings.ts");
        assert_eq!(
            super::bindings_output_path(Some(override_path)),
            std::path::PathBuf::from("/tmp/generated-bindings.ts")
        );
    }

    #[test]
    #[ignore = "explicit developer action regenerates src/bindings.ts"]
    fn export_bindings() {
        super::export_typescript_bindings(&super::specta_builder());
    }

    #[test]
    fn exported_bindings_have_no_trailing_whitespace() {
        let temp_dir = tempfile::tempdir().expect("create bindings temp directory");
        let output_path = temp_dir.path().join("bindings.ts");
        std::env::set_var("SKILL_DECK_BINDINGS_OUT", &output_path);
        super::export_typescript_bindings(&super::specta_builder());
        std::env::remove_var("SKILL_DECK_BINDINGS_OUT");

        let content = std::fs::read_to_string(output_path).expect("read generated bindings");
        let invalid_lines = content
            .lines()
            .enumerate()
            .filter_map(|(index, line)| (line.ends_with([' ', '\t'])).then_some(index + 1))
            .collect::<Vec<_>>();

        assert!(
            invalid_lines.is_empty(),
            "generated bindings contain trailing whitespace on lines {invalid_lines:?}"
        );
    }

    #[test]
    fn registered_commands_match_the_app_manifest_inventory() {
        assert_eq!(registered_commands(), app_manifest_commands());
    }

    #[test]
    fn removed_agent_definition_duplication_is_not_exported() {
        assert!(!registered_commands().contains("duplicate_custom_agent_draft"));
        assert!(!app_manifest_commands().contains("duplicate_custom_agent_draft"));
    }
}

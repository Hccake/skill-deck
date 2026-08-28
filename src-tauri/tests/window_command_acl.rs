use serde_json::Value;
use tauri::test::{get_ipc_response, mock_builder, MockRuntime, INVOKE_KEY};
use tauri::webview::InvokeRequest;
use tauri::{App, WebviewWindow, WebviewWindowBuilder};

#[tauri::command]
fn list_agents() -> &'static str {
    "list-agents"
}

#[tauri::command]
fn save_custom_agent() -> &'static str {
    "save-custom-agent"
}

#[tauri::command]
fn discover_skill_source() -> &'static str {
    "discover-skill-source"
}

#[tauri::command]
fn acquire_selected_payloads() -> &'static str {
    "acquire-selected-payloads"
}

#[tauri::command]
fn get_install_agent_selection() -> &'static str {
    "get-install-agent-selection"
}

#[tauri::command]
fn confirm_install_agent_selection() -> &'static str {
    "confirm-install-agent-selection"
}

#[tauri::command]
fn get_manage_agent_selection() -> &'static str {
    "get-manage-agent-selection"
}

#[tauri::command]
fn list_environment_projects() -> &'static str {
    "list-environment-projects"
}

#[tauri::command]
fn get_active_mutation() -> &'static str {
    "get-active-mutation"
}

#[tauri::command]
fn get_install_wizard_session() -> &'static str {
    "get-install-wizard-session"
}

#[tauri::command]
fn focus_install_wizard() -> &'static str {
    "focus-install-wizard"
}

#[tauri::command]
fn request_cancel_active_mutation() -> &'static str {
    "request-cancel-active-mutation"
}

#[tauri::command]
fn execute_lifecycle_action() -> &'static str {
    "execute-lifecycle-action"
}

#[tauri::command]
fn preview_install() -> &'static str {
    "preview-install"
}

#[tauri::command]
fn install_skills() -> &'static str {
    "install-skills"
}

#[tauri::command]
fn preview_add_library_skills() -> &'static str {
    "preview-add-library-skills"
}

#[tauri::command]
fn add_skills_to_library() -> &'static str {
    "add-skills-to-library"
}

#[tauri::command]
fn open_recovery_resource() -> &'static str {
    "open-recovery-resource"
}

#[tauri::command]
fn list_recovery_resources() -> &'static str {
    "list-recovery-resources"
}

#[tauri::command]
fn open_skill_resource() -> &'static str {
    "open-skill-resource"
}

#[tauri::command]
fn open_config_resource() -> &'static str {
    "open-config-resource"
}

#[tauri::command]
fn check_application_update() -> &'static str {
    "check-application-update"
}

#[tauri::command]
fn cancel_application_update_download() -> &'static str {
    "cancel-application-update-download"
}

#[tauri::command]
fn get_github_credential_status() -> &'static str {
    "get-github-credential-status"
}

#[tauri::command]
fn save_github_credential() -> &'static str {
    "save-github-credential"
}

#[tauri::command]
fn clear_github_credential() -> &'static str {
    "clear-github-credential"
}

#[tauri::command]
fn search_discover_skills() -> &'static str {
    "search-discover-skills"
}

#[tauri::command]
fn get_discover_leaderboard() -> &'static str {
    "get-discover-leaderboard"
}

#[tauri::command]
fn get_discover_skill_detail() -> &'static str {
    "get-discover-skill-detail"
}

#[tauri::command]
fn get_proxy_settings() -> &'static str {
    "get-proxy-settings"
}

#[tauri::command]
fn save_proxy_settings() -> &'static str {
    "save-proxy-settings"
}

#[tauri::command]
fn test_proxy_connection() -> &'static str {
    "test-proxy-connection"
}

fn test_app() -> App<MockRuntime> {
    mock_builder()
        .invoke_handler(tauri::generate_handler![
            list_agents,
            save_custom_agent,
            discover_skill_source,
            acquire_selected_payloads,
            confirm_install_agent_selection,
            get_install_agent_selection,
            get_manage_agent_selection,
            list_environment_projects,
            get_active_mutation,
            get_install_wizard_session,
            focus_install_wizard,
            request_cancel_active_mutation,
            execute_lifecycle_action,
            preview_install,
            install_skills,
            preview_add_library_skills,
            add_skills_to_library,
            list_recovery_resources,
            open_recovery_resource,
            open_skill_resource,
            open_config_resource,
            cancel_application_update_download,
            check_application_update,
            get_github_credential_status,
            save_github_credential,
            clear_github_credential,
            search_discover_skills,
            get_discover_leaderboard,
            get_discover_skill_detail,
            get_proxy_settings,
            save_proxy_settings,
            test_proxy_connection,
        ])
        .build(tauri::generate_context!())
        .expect("mock Tauri app")
}

fn window(app: &App<MockRuntime>, label: &str) -> WebviewWindow<MockRuntime> {
    WebviewWindowBuilder::new(app, label, Default::default())
        .build()
        .expect("mock webview window")
}

fn invoke(window: &WebviewWindow<MockRuntime>, command: &str) -> Result<Value, Value> {
    invoke_with_body(window, command, tauri::ipc::InvokeBody::default())
}

fn invoke_with_body(
    window: &WebviewWindow<MockRuntime>,
    command: &str,
    body: tauri::ipc::InvokeBody,
) -> Result<Value, Value> {
    let url = window.url().expect("mock webview URL");
    get_ipc_response(
        window,
        InvokeRequest {
            cmd: command.into(),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            url,
            body,
            headers: Default::default(),
            invoke_key: INVOKE_KEY.to_string(),
        },
    )
    .map(|body| body.deserialize::<Value>().expect("JSON response"))
}

#[test]
fn application_windows_do_not_receive_generic_http_plugin_permissions() {
    for (label, source) in [
        ("main", include_str!("../capabilities/main.json")),
        (
            "install-wizard",
            include_str!("../capabilities/install-wizard.json"),
        ),
    ] {
        let capability: Value = serde_json::from_str(source)
            .unwrap_or_else(|error| panic!("{label} capability must be valid JSON: {error}"));
        let permissions = capability["permissions"]
            .as_array()
            .unwrap_or_else(|| panic!("{label} permissions must be an array"));
        assert!(!permissions.iter().any(|permission| {
            let identifier = permission
                .as_str()
                .or_else(|| permission.get("identifier").and_then(Value::as_str));
            identifier.is_some_and(|identifier| {
                identifier == "http:default" || identifier.starts_with("http:allow-")
            })
        }));
    }
}

#[test]
fn only_the_main_window_can_open_external_urls() {
    let main: Value = serde_json::from_str(include_str!("../capabilities/main.json"))
        .expect("main capability must be valid JSON");
    let wizard = include_str!("../capabilities/install-wizard.json");
    let permissions = main["permissions"]
        .as_array()
        .expect("main permissions must be an array");
    let opener_permission = permissions
        .iter()
        .find(|permission| {
            permission.get("identifier").and_then(Value::as_str) == Some("opener:allow-open-url")
        })
        .expect("main window must receive the scoped external URL permission");

    assert_eq!(
        opener_permission.get("allow"),
        Some(&serde_json::json!([
            { "url": "http://*" },
            { "url": "https://*" }
        ]))
    );
    assert!(!permissions
        .iter()
        .any(|permission| { permission.as_str() == Some("opener:allow-default-urls") }));
    assert!(!wizard.contains("opener:"));
}

fn assert_denied(result: Result<Value, Value>, command: &str) {
    let error = result.expect_err("command must be denied");
    let message = error.as_str().expect("ACL denial must be a string");
    assert!(
        message.contains("not allowed"),
        "{command} returned an unexpected denial: {error}"
    );
    assert!(
        message
            .lines()
            .next()
            .is_some_and(|line| line.ends_with("URL: local")),
        "{command} must be denied from the local app origin: {error}"
    );
}

#[test]
fn main_window_applies_representative_business_command_permissions() {
    let app = test_app();
    let main = window(&app, "main");

    assert_eq!(invoke(&main, "list_agents"), Ok(Value::from("list-agents")));
    assert_eq!(
        invoke(&main, "save_custom_agent"),
        Ok(Value::from("save-custom-agent"))
    );
    assert_eq!(
        invoke(&main, "list_recovery_resources"),
        Ok(Value::from("list-recovery-resources"))
    );
    assert_eq!(
        invoke(&main, "open_recovery_resource"),
        Ok(Value::from("open-recovery-resource"))
    );
    assert_eq!(
        invoke(&main, "open_skill_resource"),
        Ok(Value::from("open-skill-resource"))
    );
    assert_eq!(
        invoke(&main, "open_config_resource"),
        Ok(Value::from("open-config-resource"))
    );
    assert_eq!(
        invoke(&main, "get_github_credential_status"),
        Ok(Value::from("get-github-credential-status"))
    );
    assert_eq!(
        invoke(&main, "save_github_credential"),
        Ok(Value::from("save-github-credential"))
    );
    assert_eq!(
        invoke(&main, "clear_github_credential"),
        Ok(Value::from("clear-github-credential"))
    );
    assert_eq!(
        invoke(&main, "get_proxy_settings"),
        Ok(Value::from("get-proxy-settings"))
    );
    assert_eq!(
        invoke(&main, "save_proxy_settings"),
        Ok(Value::from("save-proxy-settings"))
    );
    assert_eq!(
        invoke(&main, "test_proxy_connection"),
        Ok(Value::from("test-proxy-connection"))
    );
    assert_eq!(
        invoke(&main, "search_discover_skills"),
        Ok(Value::from("search-discover-skills"))
    );
    assert_eq!(
        invoke(&main, "get_discover_leaderboard"),
        Ok(Value::from("get-discover-leaderboard"))
    );
    assert_eq!(
        invoke(&main, "get_discover_skill_detail"),
        Ok(Value::from("get-discover-skill-detail"))
    );
    assert_eq!(
        invoke(&main, "discover_skill_source"),
        Ok(Value::from("discover-skill-source"))
    );
    assert_eq!(
        invoke(&main, "acquire_selected_payloads"),
        Ok(Value::from("acquire-selected-payloads"))
    );
    assert_eq!(
        invoke(&main, "preview_install"),
        Ok(Value::from("preview-install"))
    );
    assert_eq!(
        invoke(&main, "install_skills"),
        Ok(Value::from("install-skills"))
    );
    assert_eq!(
        invoke(&main, "preview_add_library_skills"),
        Ok(Value::from("preview-add-library-skills"))
    );
    assert_eq!(
        invoke(&main, "add_skills_to_library"),
        Ok(Value::from("add-skills-to-library"))
    );
    assert_eq!(
        invoke(&main, "get_install_agent_selection"),
        Ok(Value::from("get-install-agent-selection"))
    );
    assert_denied(
        invoke(&main, "confirm_install_agent_selection"),
        "confirm_install_agent_selection",
    );
    assert_eq!(
        invoke(&main, "get_manage_agent_selection"),
        Ok(Value::from("get-manage-agent-selection"))
    );
    assert_eq!(
        invoke(&main, "get_install_wizard_session"),
        Ok(Value::from("get-install-wizard-session"))
    );
    assert_eq!(
        invoke(&main, "focus_install_wizard"),
        Ok(Value::from("focus-install-wizard"))
    );
    assert_eq!(
        invoke(&main, "cancel_application_update_download"),
        Ok(Value::from("cancel-application-update-download"))
    );
}

#[test]
fn install_wizard_allows_install_discovery_but_not_settings_recovery_or_updater() {
    let app = test_app();
    let wizard = window(&app, "install-wizard");

    assert_eq!(
        invoke(&wizard, "discover_skill_source"),
        Ok(Value::from("discover-skill-source"))
    );
    assert_eq!(
        invoke(&wizard, "get_install_agent_selection"),
        Ok(Value::from("get-install-agent-selection"))
    );
    assert_eq!(
        invoke(&wizard, "confirm_install_agent_selection"),
        Ok(Value::from("confirm-install-agent-selection"))
    );
    assert_eq!(
        invoke(&wizard, "acquire_selected_payloads"),
        Ok(Value::from("acquire-selected-payloads"))
    );
    assert_eq!(
        invoke(&wizard, "preview_install"),
        Ok(Value::from("preview-install"))
    );
    assert_eq!(
        invoke(&wizard, "install_skills"),
        Ok(Value::from("install-skills"))
    );
    assert_denied(
        invoke(&wizard, "preview_add_library_skills"),
        "preview_add_library_skills",
    );
    assert_denied(
        invoke(&wizard, "add_skills_to_library"),
        "add_skills_to_library",
    );
    assert_eq!(
        invoke(&wizard, "search_discover_skills"),
        Ok(Value::from("search-discover-skills"))
    );
    assert_denied(
        invoke(&wizard, "get_discover_leaderboard"),
        "get_discover_leaderboard",
    );
    assert_denied(
        invoke(&wizard, "get_discover_skill_detail"),
        "get_discover_skill_detail",
    );
    assert_denied(invoke(&wizard, "save_custom_agent"), "save_custom_agent");
    assert_denied(invoke(&wizard, "list_agents"), "list_agents");
    assert_denied(
        invoke(&wizard, "get_manage_agent_selection"),
        "get_manage_agent_selection",
    );
    assert_denied(
        invoke(&wizard, "list_recovery_resources"),
        "list_recovery_resources",
    );
    assert_denied(
        invoke(&wizard, "open_recovery_resource"),
        "open_recovery_resource",
    );
    assert_denied(
        invoke(&wizard, "open_skill_resource"),
        "open_skill_resource",
    );
    assert_denied(
        invoke(&wizard, "open_config_resource"),
        "open_config_resource",
    );
    assert_denied(
        invoke(&wizard, "check_application_update"),
        "check_application_update",
    );
    assert_denied(
        invoke(&wizard, "cancel_application_update_download"),
        "cancel_application_update_download",
    );
    assert_denied(
        invoke(&wizard, "get_github_credential_status"),
        "get_github_credential_status",
    );
    assert_denied(
        invoke(&wizard, "save_github_credential"),
        "save_github_credential",
    );
    assert_denied(
        invoke(&wizard, "clear_github_credential"),
        "clear_github_credential",
    );
    assert_denied(invoke(&wizard, "get_proxy_settings"), "get_proxy_settings");
    assert_denied(
        invoke(&wizard, "save_proxy_settings"),
        "save_proxy_settings",
    );
    assert_denied(
        invoke(&wizard, "test_proxy_connection"),
        "test_proxy_connection",
    );
    assert_denied(
        invoke(&wizard, "get_install_wizard_session"),
        "get_install_wizard_session",
    );
    assert_denied(
        invoke(&wizard, "focus_install_wizard"),
        "focus_install_wizard",
    );
}

#[test]
fn install_wizard_allows_every_shared_runtime_command_used_by_the_flow() {
    let app = test_app();
    let wizard = window(&app, "install-wizard");

    for (command, expected) in [
        ("list_environment_projects", "list-environment-projects"),
        ("get_active_mutation", "get-active-mutation"),
        (
            "request_cancel_active_mutation",
            "request-cancel-active-mutation",
        ),
        ("execute_lifecycle_action", "execute-lifecycle-action"),
    ] {
        assert_eq!(
            invoke(&wizard, command),
            Ok(Value::from(expected)),
            "{command}"
        );
    }
}

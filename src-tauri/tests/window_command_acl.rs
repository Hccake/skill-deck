use serde_json::{json, Value};
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
fn fetch_available() -> &'static str {
    "fetch-available"
}

#[tauri::command]
fn acquire_selected_payloads() -> &'static str {
    "acquire-selected-payloads"
}

#[tauri::command]
fn get_default_target_agents() -> &'static str {
    "get-default-target-agents"
}

#[tauri::command]
fn list_agent_selection_groups() -> &'static str {
    "list-agent-selection-groups"
}

#[tauri::command]
fn list_eve_install_targets() -> &'static str {
    "list-eve-install-targets"
}

#[tauri::command]
fn check_skill_audit() -> &'static str {
    "check-skill-audit"
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
fn request_agent_configuration() -> &'static str {
    "request-agent-configuration"
}

#[tauri::command]
fn complete_agent_configuration() -> &'static str {
    "complete-agent-configuration"
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

fn test_app() -> App<MockRuntime> {
    mock_builder()
        .plugin(tauri_plugin_http::init())
        .invoke_handler(tauri::generate_handler![
            list_agents,
            save_custom_agent,
            fetch_available,
            acquire_selected_payloads,
            get_default_target_agents,
            list_agent_selection_groups,
            list_eve_install_targets,
            check_skill_audit,
            list_environment_projects,
            get_active_mutation,
            request_cancel_active_mutation,
            execute_lifecycle_action,
            preview_install,
            install_skills,
            list_recovery_resources,
            open_recovery_resource,
            open_skill_resource,
            open_config_resource,
            check_application_update,
            request_agent_configuration,
            complete_agent_configuration,
            get_github_credential_status,
            save_github_credential,
            clear_github_credential,
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
fn discover_http_is_available_only_to_main_window() {
    let app = test_app();
    let main = window(&app, "main");
    let wizard = window(&app, "install-wizard");
    let request = || {
        tauri::ipc::InvokeBody::Json(json!({
            "clientConfig": {
                "method": "GET",
                "url": "https://skills.sh/api/search?q=react&limit=1",
                "headers": []
            }
        }))
    };

    assert!(
        invoke_with_body(&main, "plugin:http|fetch", request()).is_ok(),
        "the main window must be able to start Discover HTTP requests"
    );
    assert_denied(
        invoke_with_body(&wizard, "plugin:http|fetch", request()),
        "plugin:http|fetch",
    );
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
fn main_window_allows_skill_repair_commands() {
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
        invoke(&main, "complete_agent_configuration"),
        Ok(Value::from("complete-agent-configuration"))
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
    assert_denied(
        invoke(&main, "request_agent_configuration"),
        "request_agent_configuration",
    );
    assert_eq!(
        invoke(&main, "fetch_available"),
        Ok(Value::from("fetch-available"))
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
}

#[test]
fn install_wizard_allows_install_discovery_but_not_settings_recovery_or_updater() {
    let app = test_app();
    let wizard = window(&app, "install-wizard");

    assert_eq!(
        invoke(&wizard, "fetch_available"),
        Ok(Value::from("fetch-available"))
    );
    assert_eq!(
        invoke(&wizard, "list_agents"),
        Ok(Value::from("list-agents"))
    );
    assert_eq!(
        invoke(&wizard, "get_default_target_agents"),
        Ok(Value::from("get-default-target-agents"))
    );
    assert_eq!(
        invoke(&wizard, "list_agent_selection_groups"),
        Ok(Value::from("list-agent-selection-groups"))
    );
    assert_eq!(
        invoke(&wizard, "list_eve_install_targets"),
        Ok(Value::from("list-eve-install-targets"))
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
    assert_eq!(
        invoke(&wizard, "request_agent_configuration"),
        Ok(Value::from("request-agent-configuration"))
    );
    assert_denied(
        invoke(&wizard, "complete_agent_configuration"),
        "complete_agent_configuration",
    );
    assert_denied(invoke(&wizard, "save_custom_agent"), "save_custom_agent");
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
}

#[test]
fn install_wizard_allows_every_shared_runtime_command_used_by_the_flow() {
    let app = test_app();
    let wizard = window(&app, "install-wizard");

    for (command, expected) in [
        ("check_skill_audit", "check-skill-audit"),
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

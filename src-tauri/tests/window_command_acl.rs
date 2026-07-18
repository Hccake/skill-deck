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
fn retry_runtime_maintenance() -> &'static str {
    "retry-runtime-maintenance"
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

fn test_app() -> App<MockRuntime> {
    mock_builder()
        .invoke_handler(tauri::generate_handler![
            list_agents,
            save_custom_agent,
            fetch_available,
            acquire_selected_payloads,
            get_default_target_agents,
            list_agent_selection_groups,
            list_eve_install_targets,
            preview_install,
            install_skills,
            list_recovery_resources,
            retry_runtime_maintenance,
            open_recovery_resource,
            open_skill_resource,
            open_config_resource,
            check_application_update,
            request_agent_configuration,
            complete_agent_configuration,
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
    get_ipc_response(
        window,
        InvokeRequest {
            cmd: command.into(),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            url: "tauri://localhost".parse().expect("Tauri URL"),
            body: tauri::ipc::InvokeBody::default(),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.to_string(),
        },
    )
    .map(|body| body.deserialize::<Value>().expect("JSON response"))
}

fn assert_denied(result: Result<Value, Value>, command: &str) {
    let error = result.expect_err("command must be denied");
    assert!(
        error.to_string().contains("not allowed"),
        "{command} returned an unexpected denial: {error}"
    );
}

#[test]
fn main_window_allows_settings_and_recovery_but_not_source_discovery() {
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
        invoke(&main, "retry_runtime_maintenance"),
        Ok(Value::from("retry-runtime-maintenance"))
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
    assert_denied(
        invoke(&main, "request_agent_configuration"),
        "request_agent_configuration",
    );
    assert_denied(invoke(&main, "fetch_available"), "fetch_available");
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
        invoke(&wizard, "retry_runtime_maintenance"),
        "retry_runtime_maintenance",
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
}

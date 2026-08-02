use tokio::time::Duration;

use crate::environment::wsl::protocol::{
    decode_nul_records, wsl_operation, WslOperationDescriptor, WslOperationExecutor,
    WslOperationRequest, DEFAULT_WSL_STDERR_LIMIT,
};
use crate::environment::wsl::WslSession;
use crate::error::AppError;

const EVE_PROJECT_SCRIPT: &str = include_str!("../scripts/eve.sh");
const EVE_PROJECT_OPERATION: WslOperationDescriptor =
    wsl_operation("eve-project", "inspect", EVE_PROJECT_SCRIPT);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EveProjectTargets {
    pub has_eve: bool,
    pub subagents: Vec<String>,
}

pub async fn inspect_eve_project(
    session: &WslSession,
    project_path: &str,
) -> Result<EveProjectTargets, AppError> {
    let output = WslOperationExecutor::execute(
        &EVE_PROJECT_OPERATION,
        WslOperationRequest {
            session: session.clone(),
            args: vec![project_path.to_string()],
            stdin: Vec::new(),
            timeout: Duration::from_secs(10),
            stdout_limit: 1024 * 1024,
            stderr_limit: DEFAULT_WSL_STDERR_LIMIT,
            cancellation: None,
        },
    )
    .await?;
    parse_eve_project(&output.stdout)
}

pub fn parse_eve_project(bytes: &[u8]) -> Result<EveProjectTargets, AppError> {
    let records = decode_nul_records(bytes);
    if records.first().map(String::as_str) == Some("0") {
        return Ok(EveProjectTargets {
            has_eve: false,
            subagents: Vec::new(),
        });
    }
    if records.first().map(String::as_str) != Some("1") {
        return Err(protocol_error());
    }
    let package: serde_json::Value =
        serde_json::from_str(records.get(1).ok_or_else(protocol_error)?)?;
    let has_eve = ["dependencies", "devDependencies"]
        .into_iter()
        .any(|section| {
            package
                .get(section)
                .and_then(serde_json::Value::as_object)
                .is_some_and(|dependencies| dependencies.contains_key("eve"))
        });
    let mut subagents = records.into_iter().skip(2).collect::<Vec<_>>();
    subagents.sort();
    Ok(EveProjectTargets { has_eve, subagents })
}

fn protocol_error() -> AppError {
    AppError::ConfigurationCorrupted {
        message: "invalid WSL Eve project protocol response".to_string(),
    }
}

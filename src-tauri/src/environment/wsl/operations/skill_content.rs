use tokio::time::Duration;

use crate::environment::wsl::protocol::{
    wsl_operation, WslOperationDescriptor, WslOperationExecutor, WslOperationRequest,
    DEFAULT_WSL_STDERR_LIMIT,
};
use crate::environment::wsl::WslSession;
use crate::error::AppError;

const READ_SKILL_MARKDOWN_SCRIPT: &str = include_str!("../scripts/skill-content.sh");
const SKILL_CONTENT_OPERATION: WslOperationDescriptor =
    wsl_operation("skill-content", "read", READ_SKILL_MARKDOWN_SCRIPT);

pub async fn read_skill_markdown(
    session: &WslSession,
    canonical_path: &str,
) -> Result<String, AppError> {
    let output = match WslOperationExecutor::execute(
        &SKILL_CONTENT_OPERATION,
        WslOperationRequest {
            session: session.clone(),
            args: vec![canonical_path.to_string()],
            stdin: Vec::new(),
            timeout: Duration::from_secs(10),
            stdout_limit: 4 * 1024 * 1024,
            stderr_limit: DEFAULT_WSL_STDERR_LIMIT,
            cancellation: None,
        },
    )
    .await
    {
        Ok(output) => output.stdout,
        Err(AppError::WslCommandFailed {
            exit_code: Some(44),
            ..
        }) => {
            return Err(AppError::PathNotFound {
                path: format!("{}/SKILL.md", canonical_path.trim_end_matches('/')),
            });
        }
        Err(error) => return Err(error),
    };
    String::from_utf8(output).map_err(|error| AppError::InvalidSkillMd {
        message: error.to_string(),
    })
}

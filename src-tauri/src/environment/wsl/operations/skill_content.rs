use crate::environment::wsl::WslWorkspace;
use crate::error::AppError;

const SKILL_MARKDOWN_LIMIT: u32 = 4 * 1024 * 1024;

pub async fn read_skill_markdown(
    workspace: &WslWorkspace,
    canonical_path: &str,
) -> Result<String, AppError> {
    let path = format!("{}/SKILL.md", canonical_path.trim_end_matches('/'));
    let bytes = workspace
        .read_optional_document(path.clone(), SKILL_MARKDOWN_LIMIT)
        .await?
        .ok_or(AppError::PathNotFound { path })?;
    String::from_utf8(bytes).map_err(|error| AppError::InvalidSkillMd {
        message: error.to_string(),
    })
}

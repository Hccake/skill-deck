use serde::{Deserialize, Serialize};
use specta::Type;
use tokio::time::Duration;

use crate::environment::host::inspect_host_context;
use crate::environment::types::{ContextRef, ProjectBinding, ResourceLocator};
use crate::environment::wsl::WslSession;
use crate::environment::wsl_protocol::{decode_nul_records, run_wsl_script};
use crate::error::AppError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedContext {
    pub context: ContextRef,
    pub project: Option<ProjectBinding>,
    pub home: ResourceLocator,
    pub skill_root: ResourceLocator,
    pub lock: ResourceLocator,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectRequest {
    pub context: ResolvedContext,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct SkillEntrySnapshot {
    pub name: String,
    pub canonical_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct EnvironmentSnapshot {
    pub skills: Vec<SkillEntrySnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcquireRequest {
    Git { source: String },
    HostArchive { bytes: Vec<u8> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyRequest {
    pub context: ResolvedContext,
    pub operation: String,
}

pub enum EnvironmentService {
    Host,
    Wsl(WslSession),
}

impl EnvironmentService {
    pub async fn inspect(&self, request: &InspectRequest) -> Result<EnvironmentSnapshot, AppError> {
        match self {
            Self::Host => inspect_host_context(request),
            Self::Wsl(session) => inspect_wsl_context(session, request).await,
        }
    }
}

async fn inspect_wsl_context(
    session: &WslSession,
    request: &InspectRequest,
) -> Result<EnvironmentSnapshot, AppError> {
    const SCRIPT: &str = r#"printf '1\0'; root=$1; if [ -d "$root" ]; then for dir in "$root"/*; do if [ -d "$dir" ] && [ -f "$dir/SKILL.md" ]; then name=${dir##*/}; printf '%s\0%s\0' "$name" "$dir"; fi; done; fi"#;
    let output = run_wsl_script(
        session,
        SCRIPT,
        &[request.context.skill_root.native_path.clone()],
        Vec::new(),
        Duration::from_secs(20),
    )
    .await?;
    parse_wsl_inspect_output(&output)
}

pub fn parse_wsl_inspect_output(bytes: &[u8]) -> Result<EnvironmentSnapshot, AppError> {
    let records = decode_nul_records(bytes);
    if records.first().map(String::as_str) != Some("1") || records[1..].len() % 2 != 0 {
        return Err(AppError::Custom {
            message: "invalid WSL inspect response".to_string(),
        });
    }
    let skills = records[1..]
        .chunks_exact(2)
        .map(|record| SkillEntrySnapshot {
            name: record[0].clone(),
            canonical_path: record[1].clone(),
        })
        .collect();
    Ok(EnvironmentSnapshot { skills })
}

#[cfg(test)]
mod tests {
    use super::parse_wsl_inspect_output;

    #[test]
    fn parses_versioned_wsl_inspect_records() {
        let snapshot = parse_wsl_inspect_output(
            b"1\0toolkit\0/home/alice/.agents/skills/toolkit\0review\0/home/alice/.agents/skills/review\0",
        )
        .expect("parse inspect output");

        assert_eq!(snapshot.skills.len(), 2);
        assert_eq!(snapshot.skills[0].name, "toolkit");
        assert_eq!(
            snapshot.skills[1].canonical_path,
            "/home/alice/.agents/skills/review"
        );
    }

    #[test]
    fn rejects_unknown_wsl_inspect_protocol_version() {
        assert!(parse_wsl_inspect_output(b"2\0toolkit\0/path\0").is_err());
    }
}

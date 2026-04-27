use crate::error::AppError;
use crate::models::{InstallRiskKind, InstallRiskPolicy, ParsedSource};

use super::get_owner_repo;

/// 基于解析后的 source 计算安装风险策略。
pub fn source_risk_policy(parsed: &ParsedSource) -> InstallRiskPolicy {
    let owner_repo = get_owner_repo(parsed);
    let owner = owner_repo
        .as_deref()
        .and_then(|owner_repo| owner_repo.split('/').next());

    if owner.is_some_and(|owner| owner.eq_ignore_ascii_case("openclaw")) {
        return InstallRiskPolicy {
            kind: InstallRiskKind::RequireConfirmation,
            code: Some("openclaw".to_string()),
        };
    }

    InstallRiskPolicy {
        kind: InstallRiskKind::None,
        code: None,
    }
}

/// 在真正执行安装前，强制校验需要确认的风险策略。
pub fn ensure_install_risk_acknowledged(
    risk_policy: &InstallRiskPolicy,
    acknowledged: bool,
) -> Result<(), AppError> {
    if matches!(risk_policy.kind, InstallRiskKind::RequireConfirmation) && !acknowledged {
        return Err(AppError::InstallRiskConfirmationRequired {
            code: risk_policy
                .code
                .clone()
                .unwrap_or_else(|| "unknown".to_string()),
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::parse_source;

    #[test]
    fn test_openclaw_repo_requires_explicit_confirmation() {
        let parsed = parse_source("openclaw/community-skills").expect("parse source");
        let policy = source_risk_policy(&parsed);

        assert_eq!(policy.kind, InstallRiskKind::RequireConfirmation);
        assert_eq!(policy.code.as_deref(), Some("openclaw"));
    }

    #[test]
    fn test_openclaw_owner_match_is_case_insensitive() {
        let parsed = parse_source("OpenClaw/community-skills").expect("parse source");
        let policy = source_risk_policy(&parsed);

        assert_eq!(policy.kind, InstallRiskKind::RequireConfirmation);
        assert_eq!(policy.code.as_deref(), Some("openclaw"));
    }

    #[test]
    fn test_non_guarded_source_has_no_risk_confirmation_requirement() {
        let parsed = parse_source("owner/repo").expect("parse source");
        let policy = source_risk_policy(&parsed);

        assert_eq!(policy.kind, InstallRiskKind::None);
        assert_eq!(policy.code, None);
    }
}

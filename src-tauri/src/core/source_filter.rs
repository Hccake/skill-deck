use serde::Serialize;
use sha2::{Digest, Sha256};
use specta::Type;
use url::Url;

use super::{NormalizedUpdateMetadata, SourceIdentity, SourceProvider};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SourceFilterKind {
    Git,
    WellKnown,
    Dependency,
    Local,
    Incomplete,
}

/// 仅用于列表归类；不参与来源获取、缓存或写入授权。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SourceFilterInfo {
    pub key: String,
    pub kind: SourceFilterKind,
    pub label: String,
    pub detail: String,
}

impl SourceFilterInfo {
    pub fn local() -> Self {
        Self::category("local", SourceFilterKind::Local)
    }
    pub fn incomplete() -> Self {
        Self::category("incomplete", SourceFilterKind::Incomplete)
    }

    fn category(key: &str, kind: SourceFilterKind) -> Self {
        Self {
            key: format!("source-filter:{key}"),
            kind,
            label: String::new(),
            detail: String::new(),
        }
    }

    pub fn from_metadata(metadata: &NormalizedUpdateMetadata) -> Self {
        if matches!(metadata.source_type.as_str(), "local" | "download") {
            return Self::local();
        }
        if metadata.source_type == "node_modules" {
            let package = metadata.source.trim();
            let label = if package
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"@/._-".contains(&byte))
            {
                package
            } else {
                ""
            };
            return Self {
                key: key(&format!("package:{label}")),
                kind: SourceFilterKind::Dependency,
                label: label.into(),
                detail: String::new(),
            };
        }
        let Ok(identity) = SourceIdentity::from_metadata(metadata) else {
            return Self::incomplete();
        };
        if identity.remote().provider() == &SourceProvider::Github
            || identity.remote().authority() == "gitlab.com"
        {
            return Self {
                key: key(
                    &serde_json::to_string(identity.remote()).expect("source identity serializes")
                ),
                kind: SourceFilterKind::Git,
                label: identity.remote().repository().into(),
                detail: identity.remote().authority().into(),
            };
        }
        let source = identity.acquisition().source();
        let parsed = Url::parse(source).or_else(|_| {
            let (authority, repository) = source
                .split_once(':')
                .ok_or(url::ParseError::RelativeUrlWithoutBase)?;
            let mut url = Url::parse(&format!(
                "ssh://{}",
                authority.rsplit('@').next().unwrap_or(authority)
            ))?;
            url.set_path(repository);
            Ok::<_, url::ParseError>(url)
        });
        let Ok(mut locator) = parsed else {
            return Self::incomplete();
        };
        let _ = locator.set_username("");
        let _ = locator.set_password(None);
        locator.set_fragment(None);
        let well_known = identity.remote().provider() == &SourceProvider::WellKnown;
        let path = if well_known {
            locator.path().trim_end_matches('/').to_string()
        } else {
            locator
                .path()
                .trim_end_matches('/')
                .trim_end_matches(".git")
                .to_string()
        };
        locator.set_path(&path);
        let authority = identity.remote().authority();
        Self {
            key: key(locator.as_str()),
            kind: if well_known {
                SourceFilterKind::WellKnown
            } else {
                SourceFilterKind::Git
            },
            label: format!("{authority}/{}", path.trim_start_matches('/')),
            detail: format!("{}://{authority}", locator.scheme()),
        }
    }
}

fn key(value: &str) -> String {
    format!("source-filter-v1:{:x}", Sha256::digest(value.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::NormalizedUpdateMetadata;

    fn metadata(url: &str) -> NormalizedUpdateMetadata {
        NormalizedUpdateMetadata {
            source: url.into(),
            source_url: Some(url.into()),
            source_type: "git".into(),
            ref_name: Some("main".into()),
            ref_kind: None,
            skill_path: Some("skills/demo/SKILL.md".into()),
            remote_hash: None,
            computed_hash: None,
            well_known_digest: None,
        }
    }

    #[test]
    fn repository_filter_ignores_refs_and_paths_but_keeps_update_identity_separate() {
        let one = metadata("https://github.com/Acme/Tools.git");
        let mut two = metadata("git@github.com:acme/tools.git");
        two.ref_name = Some("a".repeat(40));
        two.ref_kind = Some(crate::models::GitRefKind::Commit);
        two.skill_path = Some("other/demo/SKILL.md".into());
        assert_eq!(
            SourceFilterInfo::from_metadata(&one).key,
            SourceFilterInfo::from_metadata(&two).key
        );
        assert_ne!(
            crate::core::SourceIdentity::from_metadata(&one)
                .unwrap()
                .key(),
            crate::core::SourceIdentity::from_metadata(&two)
                .unwrap()
                .key()
        );
    }

    #[test]
    fn custom_sources_preserve_resource_identity_and_hide_credentials() {
        let one = SourceFilterInfo::from_metadata(&metadata(
            "https://user:secret@git.example:8443/Team/Repo.git?token=hidden",
        ));
        let two = SourceFilterInfo::from_metadata(&metadata(
            "https://other:password@git.example:8443/Team/Repo.git?token=hidden",
        ));
        assert_eq!(one.key, two.key);
        let display = serde_json::to_string(&one).unwrap();
        for secret in ["secret", "hidden", "password", "token="] {
            assert!(!display.contains(secret));
        }
        assert_ne!(
            one.key,
            SourceFilterInfo::from_metadata(&metadata(
                "https://git.example:8443/team/Repo.git?token=hidden"
            ))
            .key
        );
        assert_ne!(
            one.key,
            SourceFilterInfo::from_metadata(&metadata(
                "https://git.example/Team/Repo.git?token=hidden"
            ))
            .key
        );
        let mut index = metadata("https://example.com/a/index.json");
        index.source_type = "well-known".into();
        let a = SourceFilterInfo::from_metadata(&index);
        index.source_url = Some("https://example.com/b/index.json".into());
        assert_ne!(a.key, SourceFilterInfo::from_metadata(&index).key);
    }

    #[test]
    fn local_dependency_and_incomplete_sources_have_distinct_groups() {
        let mut source = metadata("not a URL");
        assert_eq!(
            SourceFilterInfo::from_metadata(&source).kind,
            SourceFilterKind::Incomplete
        );
        source.source_type = "node_modules".into();
        source.source = "@acme/tools".into();
        let package = SourceFilterInfo::from_metadata(&source);
        assert_eq!(package.kind, SourceFilterKind::Dependency);
        assert_eq!(package.label, "@acme/tools");
        source.source_type = "local".into();
        assert_eq!(
            SourceFilterInfo::from_metadata(&source),
            SourceFilterInfo::local()
        );
    }
}

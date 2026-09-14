use std::fmt;

use sha2::{Digest, Sha256};
use url::Url;

use crate::core::NormalizedUpdateMetadata;
use crate::error::AppError;
use crate::models::{ParsedSource, SourceType};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum SourceProvider {
    Github,
    Gitlab,
    Git,
    WellKnown,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
pub enum NormalizedRef {
    Default,
    Named(String),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
pub struct RemoteSourceIdentity {
    provider: SourceProvider,
    authority: String,
    repository: String,
}

impl RemoteSourceIdentity {
    pub(crate) fn from_parts(
        provider: SourceProvider,
        authority: impl Into<String>,
        repository: impl Into<String>,
    ) -> Result<Self, AppError> {
        let authority = authority.into().trim().to_ascii_lowercase();
        let repository = repository
            .into()
            .trim_matches('/')
            .trim_end_matches(".git")
            .to_string();
        if authority.is_empty() || repository.is_empty() {
            return Err(invalid_identity("remote repository identity is incomplete"));
        }
        Ok(Self {
            provider,
            authority,
            repository,
        })
    }

    #[cfg(test)]
    pub(crate) fn new(
        provider: SourceProvider,
        authority: impl Into<String>,
        repository: impl Into<String>,
    ) -> Self {
        Self::from_parts(provider, authority, repository).expect("valid test remote identity")
    }

    pub fn provider(&self) -> &SourceProvider {
        &self.provider
    }

    pub fn authority(&self) -> &str {
        &self.authority
    }

    pub fn repository(&self) -> &str {
        &self.repository
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AcquisitionTransport {
    Https,
    Ssh,
    Git,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AcquisitionTransportIdentity {
    transport: AcquisitionTransport,
    authority: String,
    repository: String,
}

impl AcquisitionTransportIdentity {
    #[cfg(test)]
    pub(crate) fn new(
        transport: AcquisitionTransport,
        authority: impl Into<String>,
        repository: impl Into<String>,
    ) -> Self {
        Self {
            transport,
            authority: authority.into(),
            repository: repository.into(),
        }
    }
}

#[derive(Clone)]
pub struct AcquisitionDescriptor {
    source: String,
    git_ref: Option<String>,
}

impl AcquisitionDescriptor {
    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn git_ref(&self) -> Option<&str> {
        self.git_ref.as_deref()
    }

    pub fn acquisition_equivalent(&self, other: &Self) -> bool {
        self.source == other.source && self.git_ref == other.git_ref
    }

    pub fn parsed_source(&self, provider: &SourceProvider) -> ParsedSource {
        ParsedSource {
            source_type: match provider {
                SourceProvider::Github => SourceType::GitHub,
                SourceProvider::Gitlab => SourceType::GitLab,
                SourceProvider::Git => SourceType::Git,
                SourceProvider::WellKnown => SourceType::WellKnown,
            },
            url: self.source.clone(),
            subpath: None,
            local_path: None,
            git_ref: self.git_ref.clone(),
            skill_filter: None,
        }
    }
}

impl fmt::Debug for AcquisitionDescriptor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AcquisitionDescriptor")
            .field("source", &"[REDACTED]")
            .field("git_ref", &self.git_ref)
            .finish()
    }
}

#[derive(Debug)]
pub struct SourceIdentity {
    remote: RemoteSourceIdentity,
    transport: AcquisitionTransportIdentity,
    normalized_ref: NormalizedRef,
    acquisition: AcquisitionDescriptor,
    display: String,
}

impl SourceIdentity {
    #[cfg(test)]
    pub fn from_parsed(parsed: &ParsedSource) -> Result<Self, AppError> {
        Self::build(
            parsed.source_type.clone(),
            parsed.url.clone(),
            parsed.git_ref.clone(),
        )
    }

    pub fn from_metadata(metadata: &NormalizedUpdateMetadata) -> Result<Self, AppError> {
        let source_type = parse_source_type(&metadata.source_type)?;
        let source = metadata
            .source_url
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| metadata.source.clone());
        let shorthand = !source.contains("://") && !source.contains('@');
        let source = if source_type == SourceType::GitHub && shorthand {
            format!("https://github.com/{source}")
        } else if source_type == SourceType::GitLab && shorthand {
            format!("https://gitlab.com/{source}")
        } else {
            source
        };
        Self::build(source_type, source, metadata.ref_name.clone())
    }

    fn build(
        source_type: SourceType,
        source: String,
        git_ref: Option<String>,
    ) -> Result<Self, AppError> {
        if matches!(source_type, SourceType::Local | SourceType::Download) {
            return Err(invalid_identity(
                "source does not have a remote Git identity",
            ));
        }
        let git_ref = if source_type == SourceType::WellKnown {
            None
        } else {
            git_ref
        };
        let location = parse_remote_location(&source, source_type == SourceType::WellKnown)?;
        let provider = provider_for(&source_type, &location.authority);
        let display = format!("{}/{}", location.authority, location.repository);
        let repository = if provider == SourceProvider::WellKnown
            || (provider != SourceProvider::Github && location.authority != "gitlab.com")
        {
            opaque_locator(&source, provider == SourceProvider::WellKnown)?
        } else if provider == SourceProvider::Github {
            location.repository.to_ascii_lowercase()
        } else {
            location.repository.clone()
        };
        let remote = RemoteSourceIdentity {
            provider,
            authority: location.authority.clone(),
            repository: repository.clone(),
        };
        let transport = AcquisitionTransportIdentity {
            transport: location.transport,
            authority: location.authority.clone(),
            repository,
        };
        let normalized_ref = match git_ref
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            Some(value) => NormalizedRef::Named(value.to_string()),
            None => NormalizedRef::Default,
        };
        Ok(Self {
            remote,
            transport,
            normalized_ref,
            acquisition: AcquisitionDescriptor { source, git_ref },
            display,
        })
    }

    pub fn remote(&self) -> &RemoteSourceIdentity {
        &self.remote
    }

    pub fn acquisition_transport(&self) -> &AcquisitionTransportIdentity {
        &self.transport
    }

    pub fn normalized_ref(&self) -> &NormalizedRef {
        &self.normalized_ref
    }

    pub fn acquisition(&self) -> &AcquisitionDescriptor {
        &self.acquisition
    }

    pub fn sanitized_display(&self) -> &str {
        &self.display
    }

    pub fn key(&self) -> String {
        let bytes = serde_json::to_vec(&(&self.remote, &self.normalized_ref))
            .expect("source identity serializes deterministically");
        format!("source-v2:{:x}", Sha256::digest(bytes))
    }
}

struct RemoteLocation {
    transport: AcquisitionTransport,
    authority: String,
    repository: String,
}

fn opaque_locator(source: &str, wellknown: bool) -> Result<String, AppError> {
    let normalized = match Url::parse(source) {
        Ok(mut url) => {
            if wellknown && !matches!(url.scheme(), "http" | "https") {
                return Err(invalid_identity("Well-known sources require HTTP or HTTPS"));
            }
            url.set_fragment(None);
            if wellknown && !url.path().trim_end_matches('/').ends_with("/index.json") {
                url.set_query(None);
                let path = url.path().trim_end_matches('/').to_string();
                url.set_path(&path);
            }
            url.to_string()
        }
        Err(_) if !wellknown => source.to_string(),
        Err(_) => return Err(invalid_identity("invalid Well-known index URL")),
    };
    Ok(format!(
        "url-v2:{:x}",
        Sha256::digest(normalized.as_bytes())
    ))
}

fn parse_remote_location(source: &str, wellknown: bool) -> Result<RemoteLocation, AppError> {
    if let Some((user_host, repository)) = source.split_once(':') {
        if !user_host.contains("//") && user_host.contains('@') {
            let authority = user_host
                .rsplit_once('@')
                .map(|(_, host)| host)
                .unwrap_or(user_host);
            return remote_location(AcquisitionTransport::Ssh, authority, repository);
        }
    }

    let url = Url::parse(source).map_err(|error| invalid_identity(&error.to_string()))?;
    let authority = match (url.host_str(), url.port()) {
        (Some(host), Some(port)) => format!("{}:{port}", host.to_ascii_lowercase()),
        (Some(host), None) => host.to_ascii_lowercase(),
        (None, _) => return Err(invalid_identity("remote URL has no authority")),
    };
    let transport = match url.scheme() {
        "http" | "https" => AcquisitionTransport::Https,
        "ssh" => AcquisitionTransport::Ssh,
        _ => AcquisitionTransport::Git,
    };
    if wellknown {
        return Ok(RemoteLocation {
            transport,
            authority,
            repository: url.path().trim_start_matches('/').to_string(),
        });
    }
    remote_location(transport, &authority, url.path())
}

fn remote_location(
    transport: AcquisitionTransport,
    authority: &str,
    repository: &str,
) -> Result<RemoteLocation, AppError> {
    let repository = repository
        .trim_matches('/')
        .trim_end_matches(".git")
        .to_string();
    if authority.is_empty() || repository.is_empty() {
        return Err(invalid_identity("remote repository identity is incomplete"));
    }
    Ok(RemoteLocation {
        transport,
        authority: authority.to_ascii_lowercase(),
        repository,
    })
}

fn provider_for(source_type: &SourceType, authority: &str) -> SourceProvider {
    if *source_type == SourceType::WellKnown {
        return SourceProvider::WellKnown;
    }
    let host = authority.split(':').next().unwrap_or(authority);
    if host.eq_ignore_ascii_case("github.com") {
        SourceProvider::Github
    } else if *source_type == SourceType::GitLab || host.to_ascii_lowercase().contains("gitlab") {
        SourceProvider::Gitlab
    } else {
        SourceProvider::Git
    }
}

fn parse_source_type(value: &str) -> Result<SourceType, AppError> {
    match value {
        "github" => Ok(SourceType::GitHub),
        "gitlab" => Ok(SourceType::GitLab),
        "git" => Ok(SourceType::Git),
        "local" => Ok(SourceType::Local),
        "well-known" | "wellknown" | "direct-url" | "directurl" => Ok(SourceType::WellKnown),
        "download" => Ok(SourceType::Download),
        _ => Err(invalid_identity("unsupported source type")),
    }
}

fn invalid_identity(reason: &str) -> AppError {
    AppError::InvalidSource {
        value: reason.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::parse_source;

    fn wellknown(source: &str) -> SourceIdentity {
        SourceIdentity::build(SourceType::WellKnown, source.to_string(), None).unwrap()
    }

    #[test]
    fn wellknown_identity_preserves_request_semantics_without_exposing_secrets() {
        let original = wellknown("https://example.com/Team/index.json?token=secret-one");
        for different in [
            "https://example.com/Team/index.json?token=secret-two",
            "http://example.com/Team/index.json?token=secret-one",
            "https://example.com:8443/Team/index.json?token=secret-one",
            "https://example.com/team/index.json?token=secret-one",
            "https://example.com/Team.git/index.json?token=secret-one",
        ] {
            assert_ne!(original.remote(), wellknown(different).remote());
        }
        assert_eq!(
            original.remote(),
            wellknown("https://EXAMPLE.com:443/Team/index.json?token=secret-one#section").remote()
        );
        let rendered = format!("{original:?} {}", original.sanitized_display());
        assert!(!rendered.contains("secret-one"));
        assert!(!rendered.contains("token="));
        assert!(original.remote().repository().starts_with("url-v2:"));
    }

    #[test]
    fn wellknown_root_and_scoped_inputs_do_not_forward_page_queries() {
        for base in ["https://example.com/", "https://example.com/Team"] {
            assert_eq!(
                wellknown(base).remote(),
                wellknown(&format!("{base}?page=2")).remote()
            );
        }
        assert_ne!(
            wellknown("https://example.com/Team").remote(),
            wellknown("https://example.com/team").remote()
        );
    }

    #[test]
    fn ordinary_git_urls_do_not_merge_distinct_transports_or_queries() {
        let identity =
            |url: &str| SourceIdentity::build(SourceType::Git, url.to_string(), None).unwrap();
        let first = identity("https://example.com/Tools.git?repo=one");
        for second in [
            "https://example.com/Tools.git?repo=two",
            "http://example.com/Tools.git?repo=one",
            "ssh://git@example.com/Tools.git",
            "https://example.com/tools.git?repo=one",
        ] {
            assert_ne!(first.remote(), identity(second).remote());
        }
    }

    #[test]
    fn github_https_and_ssh_share_remote_identity_but_not_transport() {
        let https = SourceIdentity::from_parsed(
            &parse_source("https://token@github.com/acme/tools.git#main").unwrap(),
        )
        .unwrap();
        let ssh = SourceIdentity::from_parsed(
            &parse_source("git@github.com:acme/tools.git#main").unwrap(),
        )
        .unwrap();

        assert_eq!(https.remote(), ssh.remote());
        assert_ne!(https.acquisition_transport(), ssh.acquisition_transport());
        assert_eq!(https.normalized_ref(), &NormalizedRef::Named("main".into()));
        assert!(!format!("{https:?}").contains("token"));
        assert!(!https.sanitized_display().contains("token"));
    }

    #[test]
    fn missing_ref_uses_default_without_guessing_a_branch() {
        let identity = SourceIdentity::from_parsed(
            &parse_source("https://gitlab.com/acme/tools.git").unwrap(),
        )
        .unwrap();

        assert_eq!(identity.normalized_ref(), &NormalizedRef::Default);
        assert_eq!(identity.remote().authority(), "gitlab.com");
        assert_eq!(identity.remote().repository(), "acme/tools");
    }

    #[test]
    fn acquisition_descriptor_restores_typed_source_without_reparsing_the_url() {
        let metadata = crate::core::NormalizedUpdateMetadata {
            source: "git://127.0.0.1:9418/tools.git".into(),
            source_type: "git".into(),
            source_url: Some("git://127.0.0.1:9418/tools.git".into()),
            ref_name: Some("release/v1".into()),
            skill_path: Some("skills/demo/SKILL.md".into()),
            remote_hash: None,
            computed_hash: Some("installed-hash".into()),
            well_known_digest: None,
        };
        let identity = SourceIdentity::from_metadata(&metadata).unwrap();

        let parsed = identity
            .acquisition()
            .parsed_source(identity.remote().provider());

        assert_eq!(parsed.source_type, SourceType::Git);
        assert_eq!(parsed.url, "git://127.0.0.1:9418/tools.git");
        assert_eq!(parsed.git_ref.as_deref(), Some("release/v1"));
    }

    #[test]
    fn metadata_url_secrets_never_enter_keys_display_or_debug() {
        let metadata = crate::core::NormalizedUpdateMetadata {
            source: "acme/tools".into(),
            source_type: "git".into(),
            source_url: Some("https://alice:secret@example.com/acme/tools.git?token=hidden".into()),
            ref_name: None,
            skill_path: Some("skills/demo".into()),
            remote_hash: Some("old".into()),
            computed_hash: None,
            well_known_digest: None,
        };
        let identity = SourceIdentity::from_metadata(&metadata).unwrap();
        let rendered = format!("{identity:?} {}", identity.sanitized_display());

        assert!(!rendered.contains("alice"));
        assert!(!rendered.contains("secret"));
        assert!(!rendered.contains("hidden"));
        assert!(identity.remote().repository().starts_with("url-v2:"));
        assert_eq!(identity.sanitized_display(), "example.com/acme/tools");
    }
}

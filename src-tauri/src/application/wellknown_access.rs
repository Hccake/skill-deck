use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

use crate::core::mutation::CancellationSignal;
use crate::error::AppError;

#[derive(Debug, Clone)]
pub(crate) struct WellKnownFetchResult {
    pub(crate) repo_path: PathBuf,
    pub(crate) trust_metadata: HashMap<String, WellKnownTrustMetadata>,
}

#[derive(Debug)]
pub(crate) enum WellKnownFetchError {
    Unproven(AppError),
    CatalogEstablished(AppError),
}

impl WellKnownFetchError {
    pub(crate) fn unproven(error: AppError) -> Self {
        Self::Unproven(error)
    }

    pub(crate) fn catalog_established(error: AppError) -> Self {
        Self::CatalogEstablished(error)
    }

    pub(crate) fn allows_direct_download(&self) -> bool {
        matches!(self, Self::Unproven(_))
    }

    pub(crate) fn into_error(self) -> AppError {
        match self {
            Self::Unproven(error) | Self::CatalogEstablished(error) => error,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct WellKnownIndexEvidence {
    pub(crate) index_url: String,
    pub(crate) complete_skill_catalog: Vec<String>,
    pub(crate) digests: HashMap<String, String>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct WellKnownTrustMetadata {
    pub(crate) well_known_version: Option<String>,
    pub(crate) well_known_entry_type: Option<String>,
    pub(crate) artifact_url_host: Option<String>,
    pub(crate) digest_verified: Option<bool>,
    pub(crate) trust_reason: Option<String>,
    pub(crate) artifact_url: Option<String>,
    pub(crate) digest: Option<String>,
}

pub(crate) fn extract_hostname(value: &str) -> Option<String> {
    url::Url::parse(value)
        .ok()?
        .host_str()
        .map(|host| host.strip_prefix("www.").unwrap_or(host).to_string())
}

pub(crate) type WellKnownFetchFuture<'a> =
    Pin<Box<dyn Future<Output = Result<WellKnownFetchResult, WellKnownFetchError>> + Send + 'a>>;
pub(crate) type WellKnownCheckFuture<'a> =
    Pin<Box<dyn Future<Output = Result<WellKnownIndexEvidence, AppError>> + Send + 'a>>;

pub(crate) trait WellKnownAccess: Send + Sync {
    fn fetch<'a>(
        &'a self,
        url: &'a str,
        cancellation: &'a CancellationSignal,
    ) -> WellKnownFetchFuture<'a>;

    fn check<'a>(
        &'a self,
        _url: &'a str,
        _skill_names: &'a [String],
        _cancellation: &'a CancellationSignal,
    ) -> WellKnownCheckFuture<'a> {
        Box::pin(async {
            Err(AppError::ExecutionFailed {
                message: "Well-known update evidence is unavailable".to_string(),
            })
        })
    }
}

#[cfg(test)]
pub(crate) struct UnavailableWellKnownAccess;

#[cfg(test)]
impl WellKnownAccess for UnavailableWellKnownAccess {
    fn fetch<'a>(
        &'a self,
        _url: &'a str,
        _cancellation: &'a CancellationSignal,
    ) -> WellKnownFetchFuture<'a> {
        Box::pin(async {
            Err(WellKnownFetchError::unproven(AppError::ExecutionFailed {
                message: "Well-known access is unavailable in this test".to_string(),
            }))
        })
    }
}

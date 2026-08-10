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

#[derive(Debug, Clone, Default)]
pub(crate) struct WellKnownTrustMetadata {
    pub(crate) well_known_version: Option<String>,
    pub(crate) well_known_entry_type: Option<String>,
    pub(crate) artifact_url_host: Option<String>,
    pub(crate) digest_verified: Option<bool>,
    pub(crate) trust_reason: Option<String>,
}

pub(crate) fn extract_hostname(value: &str) -> Option<String> {
    url::Url::parse(value)
        .ok()?
        .host_str()
        .map(|host| host.strip_prefix("www.").unwrap_or(host).to_string())
}

pub(crate) type WellKnownFetchFuture<'a> =
    Pin<Box<dyn Future<Output = Result<WellKnownFetchResult, AppError>> + Send + 'a>>;

pub(crate) trait WellKnownAccess: Send + Sync {
    fn fetch<'a>(
        &'a self,
        url: &'a str,
        cancellation: &'a CancellationSignal,
    ) -> WellKnownFetchFuture<'a>;
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
            Err(AppError::ExecutionFailed {
                message: "Well-known access is unavailable in this test".to_string(),
            })
        })
    }
}

use std::future::Future;
use std::pin::Pin;

use crate::application::source_acquisition::FetchResult;
use crate::application::source_acquisition::SourceDiscoveryPolicy;
use crate::core::mutation::CancellationSignal;
use crate::error::AppError;
use crate::models::ParsedSource;

pub(crate) type WslSourceFuture<'a> =
    Pin<Box<dyn Future<Output = Result<FetchResult, AppError>> + Send + 'a>>;

pub(crate) trait WslSourceAccess: Send + Sync {
    fn discover<'a>(
        &'a self,
        distro_name: &'a str,
        parsed: ParsedSource,
        requested_source: String,
        policy: SourceDiscoveryPolicy,
        cancellation: CancellationSignal,
    ) -> WslSourceFuture<'a>;
}

#[cfg(test)]
pub(crate) struct UnavailableWslSourceAccess;

#[cfg(test)]
impl WslSourceAccess for UnavailableWslSourceAccess {
    fn discover<'a>(
        &'a self,
        _distro_name: &'a str,
        _parsed: ParsedSource,
        _requested_source: String,
        _policy: SourceDiscoveryPolicy,
        _cancellation: CancellationSignal,
    ) -> WslSourceFuture<'a> {
        Box::pin(async {
            Err(AppError::ExecutionFailed {
                message: "WSL source access is unavailable in this test".to_string(),
            })
        })
    }
}

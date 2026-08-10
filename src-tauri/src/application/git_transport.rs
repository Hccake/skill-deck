use crate::core::mutation::CancellationSignal;
use crate::core::{CloneProgress, CloneResult};
use crate::error::AppError;

/// Git source 的 process transport seam。
///
/// Application use case 通过这个 interface 执行 clone 与 ref probe；生产 adapter
/// 调用系统 Git，测试 adapter 则可以把公开来源映射到确定性的本地仓库。
pub(crate) trait GitSourceTransport: Send + Sync {
    fn clone_source(
        &self,
        url: &str,
        git_ref: Option<&str>,
        on_progress: &(dyn Fn(CloneProgress) + Send + Sync),
        cancellation: CancellationSignal,
    ) -> Result<CloneResult, AppError>;

    fn probe_ref_revision(
        &self,
        url: &str,
        git_ref: Option<&str>,
        cancellation: CancellationSignal,
    ) -> Result<String, AppError>;
}

#[cfg(test)]
pub(crate) struct UnavailableGitSourceTransport;

#[cfg(test)]
impl GitSourceTransport for UnavailableGitSourceTransport {
    fn clone_source(
        &self,
        _url: &str,
        _git_ref: Option<&str>,
        _on_progress: &(dyn Fn(CloneProgress) + Send + Sync),
        _cancellation: CancellationSignal,
    ) -> Result<CloneResult, AppError> {
        Err(AppError::ExecutionFailed {
            message: "Git source access is unavailable in this test".to_string(),
        })
    }

    fn probe_ref_revision(
        &self,
        _url: &str,
        _git_ref: Option<&str>,
        _cancellation: CancellationSignal,
    ) -> Result<String, AppError> {
        Err(AppError::ExecutionFailed {
            message: "Git source access is unavailable in this test".to_string(),
        })
    }
}

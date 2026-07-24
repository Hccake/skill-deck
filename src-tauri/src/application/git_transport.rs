use crate::core::mutation::CancellationSignal;
use crate::core::{
    clone_repo_with_progress, probe_remote_ref_revision, CloneProgress, CloneResult,
};
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

#[derive(Debug, Default)]
pub(crate) struct ProcessGitTransport;

impl GitSourceTransport for ProcessGitTransport {
    fn clone_source(
        &self,
        url: &str,
        git_ref: Option<&str>,
        on_progress: &(dyn Fn(CloneProgress) + Send + Sync),
        cancellation: CancellationSignal,
    ) -> Result<CloneResult, AppError> {
        clone_repo_with_progress(url, git_ref, on_progress, cancellation)
    }

    fn probe_ref_revision(
        &self,
        url: &str,
        git_ref: Option<&str>,
        cancellation: CancellationSignal,
    ) -> Result<String, AppError> {
        probe_remote_ref_revision(url, git_ref, cancellation)
    }
}

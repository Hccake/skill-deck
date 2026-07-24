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

#[cfg(test)]
mod tests {
    use super::{GitSourceTransport, ProcessGitTransport};
    use crate::core::mutation::CancellationSignal;
    use crate::git_fixture::BareSkillRepo;

    #[test]
    fn process_transport_clones_and_probes_a_local_file_remote() {
        let remote = BareSkillRepo::new(&["skills/alpha"]);
        let transport = ProcessGitTransport;
        let source = remote.local_source();

        let initial_revision = transport
            .probe_ref_revision(&source, Some("main"), CancellationSignal::default())
            .expect("probe initial revision");
        let cloned = transport
            .clone_source(
                &source,
                Some("main"),
                &|_| {},
                CancellationSignal::default(),
            )
            .expect("clone local remote");

        assert!(cloned.repo_path.join("skills/alpha/SKILL.md").is_file());
        assert_eq!(
            cloned.ref_revision.as_deref(),
            Some(initial_revision.as_str())
        );

        remote.publish_change("skills/alpha");
        let changed_revision = transport
            .probe_ref_revision(&source, Some("main"), CancellationSignal::default())
            .expect("probe changed revision");
        assert_ne!(changed_revision, initial_revision);
    }
}

use std::future::Future;
use std::pin::Pin;

use crate::core::GithubTreeFetchOutcome;

pub(crate) type GithubTreeFuture<'a> =
    Pin<Box<dyn Future<Output = GithubTreeFetchOutcome> + Send + 'a>>;

pub(crate) trait GithubTreeAccess: Send + Sync {
    fn fetch_tree<'a>(
        &'a self,
        repository: &'a str,
        git_ref: &'a str,
        validation: Option<&'a str>,
    ) -> GithubTreeFuture<'a>;
}

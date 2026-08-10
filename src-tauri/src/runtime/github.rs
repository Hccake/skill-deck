use crate::application::github_access::{GithubTreeAccess, GithubTreeFuture};
use crate::application::github_credentials::{GithubCredentialFuture, GithubTokenValidator};
use crate::runtime::github_client::GithubApiClient;

impl GithubTreeAccess for GithubApiClient {
    fn fetch_tree<'a>(
        &'a self,
        repository: &'a str,
        git_ref: &'a str,
        validation: Option<&'a str>,
    ) -> GithubTreeFuture<'a> {
        Box::pin(async move { self.fetch_tree(repository, git_ref, validation).await })
    }
}

impl GithubTokenValidator for GithubApiClient {
    fn validate<'a>(&'a self, token: &'a str) -> GithubCredentialFuture<'a> {
        Box::pin(async move { self.validate_token(token).await })
    }
}

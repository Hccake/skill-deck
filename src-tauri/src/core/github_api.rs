#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GithubTreeFailure {
    AuthenticationRequired,
    NotFoundOrUnauthorized,
    Network,
    SourceUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GithubTreeSnapshot {
    pub ref_revision: String,
    pub root_tree_revision: String,
    pub validation: Option<String>,
    pub entries: Vec<GithubTreeSnapshotEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GithubTreeSnapshotEntry {
    pub path: String,
    pub entry_type: String,
    pub revision: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GithubTreeFetchOutcome {
    Modified(GithubTreeSnapshot),
    NotModified,
    RateLimited { retry_at_epoch_ms: Option<u64> },
    Incomplete,
    Failed(GithubTreeFailure),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GithubTokenValidation {
    Verified {
        login: String,
        rate_limit_remaining: Option<u64>,
        rate_limit_limit: Option<u64>,
        rate_limit_reset_at_epoch_ms: Option<u64>,
    },
    Invalid,
    RateLimited {
        retry_at_epoch_ms: Option<u64>,
    },
    Unavailable,
}

pub trait GithubTokenProvider: Send + Sync {
    fn token(&self) -> Option<String>;
}

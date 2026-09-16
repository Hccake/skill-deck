use std::time::Duration;

use futures_util::{stream, StreamExt};

use crate::environment::wsl::WslWorkspace;
use crate::error::AppError;

const PATH_METADATA_DEADLINE_MILLIS: u64 = 20_000;
const PATH_METADATA_AGGREGATE_LIMIT: u32 = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathMetadataQuery {
    pub path: String,
    pub content_limit: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathMetadataKind {
    Missing,
    Directory,
    SymlinkDirectory,
    SymlinkOther,
    Other,
    BrokenLink,
    Inaccessible,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathMetadataContent {
    NotRequested,
    Empty,
    Unreadable,
    Bytes(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathMetadataFact {
    pub path: String,
    pub kind: PathMetadataKind,
    pub content: PathMetadataContent,
    pub truncated: bool,
}

impl WslWorkspace {
    pub(crate) async fn inspect_path_metadata(
        &self,
        queries: Vec<PathMetadataQuery>,
    ) -> Result<Vec<PathMetadataFact>, AppError> {
        if queries.is_empty()
            || queries.iter().any(|query| {
                !query.path.starts_with('/')
                    || query.content_limit.is_some_and(|limit| {
                        limit == 0 || limit > environment_protocol::MAX_PATH_CONTENT_BYTES_PER_FILE
                    })
            })
        {
            return Err(AppError::Validation {
                field: Some("pathMetadata.queries".to_string()),
                message: "WSL path metadata requires absolute paths".to_string(),
            });
        }
        let deadline =
            tokio::time::Instant::now() + Duration::from_millis(PATH_METADATA_DEADLINE_MILLIS);
        let mut pages = Vec::new();
        let mut page_start = 0;
        while page_start < queries.len() {
            let page_end = metadata_page_end(&queries, page_start);
            pages.push(queries[page_start..page_end].to_vec());
            page_start = page_end;
        }
        let mut responses = stream::iter(pages.into_iter().enumerate().map(|(index, batch)| {
            let workspace = self.clone();
            async move { inspect_metadata_page(&workspace, index, batch, deadline).await }
        }))
        .buffer_unordered(environment_protocol::MAX_CONCURRENT_READ_REQUESTS)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<Result<Vec<_>, AppError>>()?;
        responses.sort_by_key(|(index, _)| *index);
        let facts = responses.into_iter().flat_map(|(_, facts)| facts).collect();
        Ok(facts)
    }
}

async fn inspect_metadata_page(
    workspace: &WslWorkspace,
    index: usize,
    batch: Vec<PathMetadataQuery>,
    deadline: tokio::time::Instant,
) -> Result<(usize, Vec<PathMetadataFact>), AppError> {
    let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
    let deadline_millis = u64::try_from(remaining.as_millis())
        .unwrap_or(environment_protocol::MAX_REQUEST_DEADLINE_MILLIS)
        .min(environment_protocol::MAX_REQUEST_DEADLINE_MILLIS);
    if deadline_millis == 0 {
        return Err(AppError::WslCommandTimedOut);
    }
    let response: environment_protocol::PathMetadataResponse = tokio::time::timeout_at(
        deadline,
        workspace.request_worker_payload(environment_protocol::Message::InspectPaths {
            request: environment_protocol::PathMetadataRequest {
                queries: batch
                    .iter()
                    .map(|query| environment_protocol::PathMetadataQuery {
                        path: query.path.clone(),
                        content_limit: query.content_limit,
                    })
                    .collect(),
                aggregate_content_limit: PATH_METADATA_AGGREGATE_LIMIT,
                deadline_millis,
            },
        }),
    )
    .await
    .map_err(|_| AppError::WslCommandTimedOut)??;
    if response.facts.len() != batch.len()
        || response
            .facts
            .iter()
            .zip(&batch)
            .any(|(fact, query)| fact.path != query.path)
    {
        return Err(protocol_error());
    }
    Ok((
        index,
        response
            .facts
            .into_iter()
            .map(|fact| PathMetadataFact {
                path: fact.path,
                kind: match fact.kind {
                    environment_protocol::PathMetadataKind::Missing => PathMetadataKind::Missing,
                    environment_protocol::PathMetadataKind::Directory => {
                        PathMetadataKind::Directory
                    }
                    environment_protocol::PathMetadataKind::SymlinkDirectory => {
                        PathMetadataKind::SymlinkDirectory
                    }
                    environment_protocol::PathMetadataKind::SymlinkOther => {
                        PathMetadataKind::SymlinkOther
                    }
                    environment_protocol::PathMetadataKind::Other => PathMetadataKind::Other,
                    environment_protocol::PathMetadataKind::BrokenLink => {
                        PathMetadataKind::BrokenLink
                    }
                    environment_protocol::PathMetadataKind::Inaccessible => {
                        PathMetadataKind::Inaccessible
                    }
                },
                content: match fact.content {
                    environment_protocol::PathMetadataContent::NotRequested => {
                        PathMetadataContent::NotRequested
                    }
                    environment_protocol::PathMetadataContent::Empty => PathMetadataContent::Empty,
                    environment_protocol::PathMetadataContent::Unreadable => {
                        PathMetadataContent::Unreadable
                    }
                    environment_protocol::PathMetadataContent::Bytes(bytes) => {
                        PathMetadataContent::Bytes(bytes)
                    }
                },
                truncated: fact.content_truncated,
            })
            .collect(),
    ))
}

fn metadata_page_end(queries: &[PathMetadataQuery], start: usize) -> usize {
    let mut end = start;
    let mut content_budget = 0u32;
    while end < queries.len() && end - start < environment_protocol::MAX_INSPECTION_ROOTS {
        let requested = queries[end].content_limit.unwrap_or_default();
        if end > start && content_budget.saturating_add(requested) > PATH_METADATA_AGGREGATE_LIMIT {
            break;
        }
        content_budget = content_budget.saturating_add(requested);
        end += 1;
    }
    end
}

fn protocol_error() -> AppError {
    AppError::ConfigurationCorrupted {
        message: "invalid WSL Worker path metadata response".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn queries(count: usize, content_limit: Option<u32>) -> Vec<PathMetadataQuery> {
        (0..count)
            .map(|index| PathMetadataQuery {
                path: format!("/tmp/{index}"),
                content_limit,
            })
            .collect()
    }

    #[test]
    fn metadata_pages_respect_count_and_worst_case_content_limits() {
        let frontmatter = queries(100, Some(256 * 1024));
        assert_eq!(metadata_page_end(&frontmatter, 0), 32);
        assert_eq!(metadata_page_end(&frontmatter, 32), 64);

        let facts_only = queries(300, None);
        assert_eq!(metadata_page_end(&facts_only, 0), 256);
        assert_eq!(metadata_page_end(&facts_only, 256), 300);
    }
}

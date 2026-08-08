use std::collections::{BTreeSet, HashMap};
use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};

use crate::core::agent_definition::AgentId;
use crate::environment::runtime::ContextSnapshotRevision;
use crate::environment::types::{
    same_environment_identity, EnvironmentRef, ResourceLocator, SkillLocationRef,
};
use crate::error::AppError;

pub const MAX_FRONTMATTER_BYTES_PER_FILE: u32 = 256 * 1024;
pub const MAX_FRONTMATTER_BYTES_PER_PLAN: u32 = 8 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ReadRootPurpose {
    Context,
    Canonical,
    #[cfg_attr(not(test), allow(dead_code))]
    Detection,
    Private,
    Adapter,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadRoot {
    pub locator: ResourceLocator,
    pub purposes: BTreeSet<ReadRootPurpose>,
    pub consumer_agent_ids: BTreeSet<AgentId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadPlan {
    pub context: SkillLocationRef,
    pub roots: Vec<ReadRoot>,
    pub registry_revision: String,
    pub environment_revision: String,
    pub context_revision: ContextSnapshotRevision,
    pub per_file_limit: u32,
    pub aggregate_limit: u32,
}

pub struct ReadPlanBuilder {
    context: SkillLocationRef,
    roots: HashMap<ResourceLocator, ReadRoot>,
    registry_revision: String,
    environment_revision: String,
    context_revision: ContextSnapshotRevision,
}

impl ReadPlanBuilder {
    pub fn new(
        context: SkillLocationRef,
        registry_revision: impl Into<String>,
        environment_revision: impl Into<String>,
        context_revision: ContextSnapshotRevision,
    ) -> Self {
        Self {
            context,
            roots: HashMap::new(),
            registry_revision: registry_revision.into(),
            environment_revision: environment_revision.into(),
            context_revision,
        }
    }

    pub fn add_root(
        &mut self,
        locator: ResourceLocator,
        purpose: ReadRootPurpose,
        consumer: Option<AgentId>,
    ) -> Result<(), AppError> {
        if !same_environment_identity(&locator.environment, &self.context.environment) {
            return Err(AppError::Validation {
                field: Some("readRoot.environment".to_string()),
                message: "ReadPlan root belongs to another Environment".to_string(),
            });
        }
        let root = self
            .roots
            .entry(locator.clone())
            .or_insert_with(|| ReadRoot {
                locator,
                purposes: BTreeSet::new(),
                consumer_agent_ids: BTreeSet::new(),
            });
        root.purposes.insert(purpose);
        if let Some(consumer) = consumer {
            root.consumer_agent_ids.insert(consumer);
        }
        Ok(())
    }

    pub fn build(self) -> Result<ReadPlan, AppError> {
        if self.roots.is_empty() {
            return Err(AppError::Validation {
                field: Some("readPlan.roots".to_string()),
                message: "ReadPlan requires at least one root".to_string(),
            });
        }
        let mut roots = self.roots.into_values().collect::<Vec<_>>();
        roots.sort_by(|left, right| left.locator.native_path.cmp(&right.locator.native_path));
        Ok(ReadPlan {
            context: self.context,
            roots,
            registry_revision: self.registry_revision,
            environment_revision: self.environment_revision,
            context_revision: self.context_revision,
            per_file_limit: MAX_FRONTMATTER_BYTES_PER_FILE,
            aggregate_limit: MAX_FRONTMATTER_BYTES_PER_PLAN,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FilesystemEntryKind {
    Missing,
    File,
    Directory,
    Symlink,
    ReparsePoint,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawPathFact {
    pub root_index: u32,
    pub relative_path: String,
    pub kind: FilesystemEntryKind,
    pub resolved_target: Option<String>,
    pub frontmatter_bytes: Vec<u8>,
    pub truncated: bool,
    pub error_code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawFilesystemSnapshot {
    pub environment: EnvironmentRef,
    pub facts: Vec<RawPathFact>,
    pub total_content_bytes: u32,
}

pub type InspectionFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait FilesystemInspector: Send + Sync {
    fn environment(&self) -> EnvironmentRef;

    fn inspect<'a>(
        &'a self,
        plan: &'a ReadPlan,
    ) -> InspectionFuture<'a, Result<RawFilesystemSnapshot, AppError>>;
}

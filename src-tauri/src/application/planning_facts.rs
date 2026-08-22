use std::future::Future;
use std::pin::Pin;

use crate::application::mutation::plan::RuntimeRevisions;
use crate::core::lossless_lock::{LockSchema, LosslessLockDocument};
use crate::environment::agent_environment::AgentRuntimeSnapshot;
use crate::environment::context_resolver::ResolvedContext;
use crate::environment::types::SkillLocationRef;
use crate::error::AppError;
use crate::models::InstallTargetInfo;

pub type ScopePlanningFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Clone)]
pub struct ScopePlanningSnapshot {
    pub resolved_context: ResolvedContext,
    pub agent_runtime: AgentRuntimeSnapshot,
    pub revisions: RuntimeRevisions,
    pub lock_schema: LockSchema,
    pub lock_document: LosslessLockDocument,
    pub eve_targets: Vec<InstallTargetInfo>,
}

pub trait ScopePlanningSnapshotSource: Send + Sync {
    fn snapshot<'a>(
        &'a self,
        context: &'a SkillLocationRef,
    ) -> ScopePlanningFuture<'a, Result<ScopePlanningSnapshot, AppError>>;

    fn copy_source_snapshot<'a>(
        &'a self,
        context: &'a SkillLocationRef,
    ) -> ScopePlanningFuture<'a, Result<ScopePlanningSnapshot, AppError>> {
        self.snapshot(context)
    }
}

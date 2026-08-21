use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use specta::Type;
use tokio::sync::Notify;

use crate::environment::types::{EnvironmentRef, SkillLocation};

pub const MUTATION_STATE_CHANGED_EVENT: &str = "mutation-state-changed";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum MutationKind {
    Install,
    Update,
    Remove,
    Copy,
    ManageAgents,
    // Kept so persisted recovery records from the retired cleanup operation remain readable.
    #[allow(dead_code)]
    DuplicateCleanup,
    Repair,
    ManageAgentDefinitions,
    ProjectMigration,
    AddProject,
    RemoveProject,
    UpdateProjectPreference,
    UpdateSettings,
    ManageGithubCredential,
    ManageLibraries,
    ResolveRecovery,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum MutationPhase {
    Preparing,
    Acquiring,
    Validating,
    Committing,
    Finishing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[specta(tag = "kind", rename_all = "camelCase")]
pub enum MutationTargetRef {
    SkillLocation {
        environment: EnvironmentRef,
        scope: SkillLocation,
    },
    Library {
        environment: EnvironmentRef,
        #[serde(rename = "libraryId")]
        #[specta(rename = "libraryId")]
        library_id: String,
    },
}

impl MutationTargetRef {
    pub fn environment(&self) -> &EnvironmentRef {
        match self {
            Self::SkillLocation { environment, .. } => environment,
            Self::Library { environment, .. } => environment,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct MutationProgress {
    pub subject: Option<String>,
    pub current: Option<u32>,
    pub total: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct ActiveMutation {
    pub id: String,
    pub kind: MutationKind,
    pub target: MutationTargetRef,
    pub phase: MutationPhase,
    pub progress: Option<MutationProgress>,
    pub cancelable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct MutationSnapshot {
    pub revision: u32,
    pub active: Option<ActiveMutation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum LifecycleLeaseKind {
    ApplicationUpdate,
    RuntimeMaintenance,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct ActiveLifecycleLease {
    pub id: String,
    pub kind: LifecycleLeaseKind,
    pub cancelable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct BackendActivitySnapshot {
    pub revision: u32,
    pub mutation: Option<ActiveMutation>,
    pub lifecycle: Option<ActiveLifecycleLease>,
}

#[derive(Default)]
struct CancellationState {
    cancelled: AtomicBool,
    notification: Notify,
}

#[derive(Clone, Default)]
pub struct CancellationSignal(Arc<CancellationState>);

impl CancellationSignal {
    pub fn cancel(&self) {
        if !self.0.cancelled.swap(true, Ordering::AcqRel) {
            self.0.notification.notify_waiters();
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.cancelled.load(Ordering::Acquire)
    }

    pub async fn cancelled(&self) {
        if self.is_cancelled() {
            return;
        }
        let notified = self.0.notification.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if self.is_cancelled() {
            return;
        }
        notified.await;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminationAdmission {
    Acquired,
    AlreadyRequested,
    Blocked(BackendActivitySnapshot),
}

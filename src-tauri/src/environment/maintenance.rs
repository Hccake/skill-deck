use serde::Serialize;
use specta::Type;

use crate::environment::types::EnvironmentRef;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum RuntimeMaintenanceState {
    Pending,
    Ready,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum MaintenanceIssueCode {
    PayloadSweepFailed,
    RecoveryReindexFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct RuntimeMaintenanceStatus {
    pub environment: EnvironmentRef,
    pub state: RuntimeMaintenanceState,
    pub issues: Vec<MaintenanceIssueCode>,
}

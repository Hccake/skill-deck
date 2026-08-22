use serde::{Deserialize, Serialize};
use specta::Type;

use crate::core::agent_definition::AgentId;
use crate::environment::planning::{ResolvedTargetFact, TargetEntryKind};
use crate::environment::runtime::ObservedEntryId;
use crate::environment::types::ResourceLocator;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct ObservedEntryReader {
    pub agent_id: AgentId,
    pub display_name: String,
    pub logical_target_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub enum ObservedEntryKind {
    Missing,
    Directory,
    Symlink,
    Junction,
    BrokenLink,
    Other,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(rename_all = "camelCase")]
pub struct ObservedPhysicalEntry {
    pub entry_id: ObservedEntryId,
    pub display_path: ResourceLocator,
    pub kind: ObservedEntryKind,
    pub physical_target_key: String,
    pub readers: Vec<ObservedEntryReader>,
    pub will_break_if_standard_removed: bool,
}

#[derive(Debug, Clone)]
pub struct ObservedPlannedEntry {
    pub public: ObservedPhysicalEntry,
    pub fact: ResolvedTargetFact,
}

pub fn observed_entry_kind(kind: TargetEntryKind) -> ObservedEntryKind {
    match kind {
        TargetEntryKind::Missing => ObservedEntryKind::Missing,
        TargetEntryKind::Directory => ObservedEntryKind::Directory,
        TargetEntryKind::Symlink => ObservedEntryKind::Symlink,
        TargetEntryKind::Junction => ObservedEntryKind::Junction,
        TargetEntryKind::BrokenLink => ObservedEntryKind::BrokenLink,
        TargetEntryKind::File | TargetEntryKind::Other => ObservedEntryKind::Other,
    }
}

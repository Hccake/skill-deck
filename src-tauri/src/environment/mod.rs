pub mod acquisition;
pub mod agent_environment;
pub mod content_manifest;
pub mod context_resolver;
pub mod directory_inspection;
pub mod inspection;
pub mod lock_io;
pub mod maintenance;
pub mod native;
pub mod opener;
pub mod path_mapping;
pub mod planning;
pub mod project_service;
pub mod read_service;
pub mod recovery;
pub mod runtime;
pub mod types;
pub mod wsl;
pub mod wsl_protocol;

#[allow(unused_imports)]
pub use agent_environment::{
    AgentRuntimeSnapshot, DetectionState, DirectoryPresenceState, EnvironmentContext,
    ResolvedAgent, ResolvedAgentScope, ResolvedPathPresence,
};

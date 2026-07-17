pub mod acquisition;
pub mod agent_environment;
pub mod context_resolver;
#[cfg(test)]
pub mod host;
pub mod lock_io;
pub mod materialize;
pub mod path_mapping;
pub mod service;
pub mod types;
pub mod wsl;
pub mod wsl_protocol;

#[allow(unused_imports)]
pub use agent_environment::{
    AgentRuntimeSnapshot, DetectionState, DirectoryPresenceState, EnvironmentContext,
    ResolvedAgent, ResolvedAgentScope, ResolvedPathPresence,
};

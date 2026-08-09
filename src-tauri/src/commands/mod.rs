// src-tauri/src/commands/mod.rs
pub mod agent_selection;
pub mod agents;
pub mod audit;
pub mod config;
pub mod copy_skill;
pub mod environments;
pub mod github_credentials;
pub mod install;
pub mod install_workflow;
pub mod lifecycle;
pub mod manage_agents;
pub mod mutations;
pub mod recovery;
pub mod remove;
pub mod resources;
pub mod skills;
pub mod source_acquisition;
pub mod update;
pub mod updater;
mod window_role;
pub mod wizard;

pub use agents::ManagedAgentRegistry;

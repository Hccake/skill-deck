// src-tauri/src/core/mod.rs
pub mod agents;
pub mod audit;
pub mod discovery;
pub mod git;
pub mod github_api;
pub mod installer;
pub mod paths;
pub mod skill;
pub mod local_lock;
pub mod skill_lock;
pub mod source_parser;
pub mod uninstaller;

pub use discovery::*;
pub use git::*;
pub use github_api::*;
pub use installer::*;
pub use source_parser::*;

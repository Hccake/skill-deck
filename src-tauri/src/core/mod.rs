// src-tauri/src/core/mod.rs
pub mod agents;
pub mod discovery;
pub mod git;
pub mod github_api;
pub mod installer;
pub mod paths;
pub mod skill;
pub mod skill_lock;
pub mod source_parser;
pub mod uninstaller;

pub use discovery::*;
pub use git::*;
pub use github_api::*;
pub use installer::*;
pub use installer::is_skill_installed;
pub use source_parser::*;
pub use uninstaller::remove_skill;

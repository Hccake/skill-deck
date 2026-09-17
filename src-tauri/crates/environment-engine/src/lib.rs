#![forbid(unsafe_code)]

//! Shared filesystem mechanics, with platform-specific Linux operations for the WSL Worker.

pub mod atomic_document;
pub mod directory;
pub mod document;
pub mod entry;
pub mod git_ref;
pub mod inspection;
pub mod library;
pub mod linux_mutation;
pub mod lock;
pub mod manifest;
pub mod path;
pub mod payload;
pub mod projection;
pub mod source_inventory;

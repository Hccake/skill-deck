#![forbid(unsafe_code)]

//! Shared Linux filesystem mechanics used by Native Linux and the WSL Worker.

pub mod directory;
pub mod document;
pub mod entry;
pub mod inspection;
pub mod library;
pub mod linux_mutation;
pub mod lock;
pub mod manifest;
pub mod path;
pub mod payload;
pub mod projection;
pub mod source_inventory;

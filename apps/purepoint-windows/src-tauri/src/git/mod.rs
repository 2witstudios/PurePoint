//! Git queries for the review views: branch, local and commit reviews, working-tree
//! status and diffs, paged patch reading, and pull requests through `gh` (WIN-8.1, port of
//! `Services/GitService.swift` and `Services/ProjectHost.swift`).
//!
//! - `runner`: local git/gh processes (no console window, bounded capture, cancel/timeout).
//! - `host`: where git runs — this machine, or a machine's daemon through [`host::GitExec`].
//! - `patch`: patch and hunk parsing, C-quoted paths, disk patch indexing.
//! - `service`: the per-host service, its caches and process plumbing.
//! - `review`: branch, local, commit and working-tree reviews and review pages.
//! - `pr`: PR list and PR patches.
//! - `commands`: the Tauri commands.

pub mod commands;
pub mod host;
pub mod model;
pub mod patch;
pub mod pr;
pub mod review;
pub mod runner;
pub mod service;

#[cfg(test)]
mod tests;

pub use host::{
    DaemonProjectHost, GitExec, LocalProjectHost, ProjectHost, UnavailableGitExec,
    UnavailableProjectHost,
};
pub use model::*;
pub use runner::{CancelToken, CommandResult, HostCommand};
pub use service::GitService;

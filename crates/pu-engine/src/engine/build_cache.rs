use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use crate::build_cache::{
    SWEEP_INTERVAL, free_bytes, legacy_target_dirs, remove_idle, sweep_policy, tmp_roots,
    worktree_target_dirs,
};

use super::Engine;

impl Engine {
    /// Start a background task that deletes abandoned agent build caches at
    /// startup and every [`SWEEP_INTERVAL`] after.
    pub fn start_build_cache_sweeper(self: &Arc<Self>) {
        let engine = Arc::clone(self);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(SWEEP_INTERVAL);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                let projects = engine.registered_projects();
                tokio::task::spawn_blocking(move || sweep_build_caches(&projects))
                    .await
                    .ok();
            }
        });
    }
}

fn sweep_build_caches(projects: &[String]) {
    let policy = sweep_policy(free_bytes(Path::new("/")));
    let mut dirs: Vec<PathBuf> = tmp_roots()
        .iter()
        .flat_map(|root| legacy_target_dirs(root))
        .collect();
    if policy.include_worktree_caches {
        dirs.extend(
            projects
                .iter()
                .flat_map(|p| worktree_target_dirs(Path::new(p))),
        );
    }
    let removed = remove_idle(&dirs, SystemTime::now(), policy.idle_after);
    if !removed.is_empty() {
        tracing::info!(
            count = removed.len(),
            dirs = ?removed,
            "removed idle agent build caches"
        );
    }
}

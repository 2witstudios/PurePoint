//! Tauri commands over [`Watchers`] (front end: `ui/src/services/watchers.ts`).
//!
//! Events go to every window as `manifest-changed`, `worktrees-changed`,
//! `file-tree-changed` and `machines-changed`. `watchers_release` holds the
//! [`ReleaseGuard`] under a token until the UI restores it (the request failed) or
//! discards it (the worktree is gone). Stopping a watcher waits for its handles to
//! close, so the calls run on the blocking pool.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use super::{ReleaseGuard, WatchId, WatchSpec, Watchers, machine_registry};

/// The app's watchers and the releases awaiting restore or discard.
#[derive(Clone)]
pub struct WatcherState(Arc<Shared>);

struct Shared {
    watchers: Arc<Watchers>,
    released: Mutex<Releases>,
}

#[derive(Default)]
struct Releases {
    next_token: u64,
    guards: HashMap<u64, ReleaseGuard>,
}

/// What `watchers_release` returns: the token for restore or discard, and how many
/// watchers were stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Released {
    pub token: u64,
    pub released: usize,
}

impl WatcherState {
    /// Watchers that emit to every window of `app`.
    pub fn new(app: AppHandle) -> Self {
        Self::with_watchers(Watchers::new(Arc::new(move |event| {
            let _ = app.emit(event.name(), event);
        })))
    }

    pub fn with_watchers(watchers: Arc<Watchers>) -> Self {
        Self(Arc::new(Shared {
            watchers,
            released: Mutex::new(Releases::default()),
        }))
    }

    pub fn watchers(&self) -> &Arc<Watchers> {
        &self.0.watchers
    }

    fn releases(&self) -> std::sync::MutexGuard<'_, Releases> {
        self.0.released.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Releases the watchers at or under `path` and keeps the guard under a token.
    pub fn release(&self, path: &Path) -> Released {
        let guard = self.0.watchers.release_under(path);
        let released = guard.specs().len();
        let mut releases = self.releases();
        releases.next_token += 1;
        let token = releases.next_token;
        releases.guards.insert(token, guard);
        Released { token, released }
    }

    /// Re-creates a release's watchers; 0 for an unknown token.
    pub fn restore(&self, token: u64) -> usize {
        let guard = self.releases().guards.remove(&token);
        guard.map_or(0, ReleaseGuard::restore)
    }

    /// Stops every watcher and forgets every release (the page reloaded or its renderer
    /// crashed, so nothing will restore them or unwatch). The watchers stop emitting before
    /// this returns; their handles close on the blocking pool. Returns how many were stopped.
    pub fn reset(&self) -> usize {
        let stopped = self.0.watchers.stop_all();
        self.releases().guards.clear();
        let count = stopped.len();
        if !stopped.is_empty() {
            std::thread::spawn(move || stopped.join());
        }
        count
    }

    /// The release tokens still held.
    pub fn pending_releases(&self) -> usize {
        self.releases().guards.len()
    }

    /// Forgets a release, leaving its watchers stopped; false for an unknown token.
    pub fn discard(&self, token: u64) -> bool {
        self.releases().guards.remove(&token).is_some()
    }
}

async fn blocking<T: Send + 'static>(
    state: State<'_, WatcherState>,
    work: impl FnOnce(WatcherState) -> T + Send + 'static,
) -> Result<T, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || work(state))
        .await
        .map_err(|error| error.to_string())
}

/// Watches `<projectRoot>\.pu\manifest.json`; raises `manifest-changed`.
#[tauri::command]
pub async fn watch_manifest(
    state: State<'_, WatcherState>,
    project_root: String,
) -> Result<WatchId, String> {
    crate::files::roots::global().add(&project_root);
    let spec = WatchSpec::Manifest {
        project_root: PathBuf::from(project_root),
    };
    blocking(state, move |state| state.watchers().watch(spec)).await
}

/// Watches a worktree's git metadata; raises `worktrees-changed`.
#[tauri::command]
pub async fn watch_worktree(
    state: State<'_, WatcherState>,
    worktree_path: String,
) -> Result<WatchId, String> {
    crate::files::roots::global().add(&worktree_path);
    let spec = WatchSpec::Worktree {
        worktree_path: PathBuf::from(worktree_path),
    };
    blocking(state, move |state| state.watchers().watch(spec)).await
}

/// Watches a file-tree root recursively; raises `file-tree-changed`.
#[tauri::command]
pub async fn watch_file_tree(
    state: State<'_, WatcherState>,
    root: String,
) -> Result<WatchId, String> {
    crate::files::roots::global().add(&root);
    let spec = WatchSpec::FileTree {
        root: PathBuf::from(root),
    };
    blocking(state, move |state| state.watchers().watch(spec)).await
}

/// Watches `%USERPROFILE%\.pu\machines.json`; raises `machines-changed`.
#[tauri::command]
pub async fn watch_machines(state: State<'_, WatcherState>) -> Result<WatchId, String> {
    let registry_path = machine_registry::default_registry_path().map_err(|e| e.to_string())?;
    let spec = WatchSpec::Machines { registry_path };
    blocking(state, move |state| state.watchers().watch(spec)).await
}

/// Stops a watcher; false if `id` is unknown.
#[tauri::command]
pub async fn unwatch(state: State<'_, WatcherState>, id: WatchId) -> Result<bool, String> {
    blocking(state, move |state| state.watchers().unwatch(id)).await
}

/// Stops every watcher at or under `path` and waits for its handles to close (WIN-3.13).
#[tauri::command]
pub async fn watchers_release(
    state: State<'_, WatcherState>,
    path: String,
) -> Result<Released, String> {
    blocking(state, move |state| state.release(Path::new(&path))).await
}

/// Re-creates a release's watchers after the request that needed it failed.
#[tauri::command]
pub async fn watchers_restore(state: State<'_, WatcherState>, token: u64) -> Result<usize, String> {
    blocking(state, move |state| state.restore(token)).await
}

/// Forgets a release after the request that needed it succeeded.
#[tauri::command]
pub async fn watchers_discard(state: State<'_, WatcherState>, token: u64) -> Result<bool, String> {
    blocking(state, move |state| state.discard(token)).await
}

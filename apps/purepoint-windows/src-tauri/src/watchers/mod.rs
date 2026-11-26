//! Project watchers (WIN-5.6), ported from the Swift GCD `DispatchSource` watchers:
//! `ManifestWatcher`, `WorktreeWatcher`, `FileTreeWatcher` and `MachineRegistryWatcher`.
//!
//! Each watcher is a `notify` watcher owned by one thread that debounces its events
//! (trailing edge: every relevant event restarts the quiet period, as the Swift
//! `DispatchWorkItem` debounce does) and then raises one [`WatchEvent`]. After every
//! burst the thread recomputes the directories it should watch and re-arms if they
//! changed, which covers the Swift re-open-after-atomic-replace and retry paths; with
//! nothing to watch it polls once a second, like `MachineRegistryWatcher`'s retry timer.
//!
//! The WIN-3.13 release contract (REQ-WINAPP-043): [`Watchers::release_under`] drops
//! every watcher whose root is at or under a path, waits until `notify` has closed its
//! directory handles, and returns a [`ReleaseGuard`] that re-creates them.

pub mod commands;
pub mod file_tree;
pub mod machine_registry;
pub mod manifest;
pub mod worktree;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;

/// How often a watcher with nothing to watch looks for its paths again.
const RETRY_INTERVAL: Duration = Duration::from_secs(1);
/// How long a release waits for `notify` to close its handles.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(2);

/// What a watcher raises; `name()` is the Tauri event, the value its payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged, rename_all_fields = "camelCase")]
pub enum WatchEvent {
    /// `.pu/manifest.json` changed (ProjectState refreshes).
    ManifestChanged { project_root: String },
    /// A worktree's git metadata (HEAD, index, refs) changed (DiffState refreshes).
    WorktreesChanged { worktree_path: String },
    /// Entries under a file-tree root changed (FileTreeState refreshes).
    FileTreeChanged { root: String, paths: Vec<String> },
    /// `~/.pu/machines.json` changed (AppState reloads machines).
    MachinesChanged {},
}

impl WatchEvent {
    pub fn name(&self) -> &'static str {
        match self {
            Self::ManifestChanged { .. } => "manifest-changed",
            Self::WorktreesChanged { .. } => "worktrees-changed",
            Self::FileTreeChanged { .. } => "file-tree-changed",
            Self::MachinesChanged {} => "machines-changed",
        }
    }
}

/// What to watch. `root()` is the path the release contract matches against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchSpec {
    Manifest { project_root: PathBuf },
    Worktree { worktree_path: PathBuf },
    FileTree { root: PathBuf },
    Machines { registry_path: PathBuf },
}

impl WatchSpec {
    pub fn root(&self) -> &Path {
        match self {
            Self::Manifest { project_root } => project_root,
            Self::Worktree { worktree_path } => worktree_path,
            Self::FileTree { root } => root,
            Self::Machines { registry_path } => registry_path,
        }
    }

    fn source(&self) -> Box<dyn Source> {
        match self {
            Self::Manifest { project_root } => {
                Box::new(manifest::ManifestSource::new(project_root))
            }
            Self::Worktree { worktree_path } => {
                Box::new(worktree::WorktreeSource::new(worktree_path))
            }
            Self::FileTree { root } => Box::new(file_tree::FileTreeSource::new(root)),
            Self::Machines { registry_path } => {
                Box::new(machine_registry::MachineRegistrySource::new(registry_path))
            }
        }
    }
}

/// One kind of watcher: where to watch, which paths count, and what to raise.
pub(crate) trait Source: Send + 'static {
    /// The quiet period that ends a burst.
    fn debounce(&self) -> Duration;
    /// The directories to watch now; recomputed after every burst.
    fn targets(&self) -> Vec<(PathBuf, RecursiveMode)>;
    /// Whether a changed path belongs to this watcher.
    fn relevant(&self, path: &Path) -> bool;
    /// The event for a burst of relevant paths.
    fn event(&self, paths: Vec<PathBuf>) -> WatchEvent;
}

pub type WatchId = u64;
pub type Emit = Arc<dyn Fn(&WatchEvent) + Send + Sync>;

enum Msg {
    Fs(notify::Result<notify::Event>),
    Stop,
}

struct Running {
    control: Sender<Msg>,
    thread: JoinHandle<()>,
}

impl Running {
    fn start(spec: &WatchSpec, emit: Emit) -> Self {
        let (control, rx) = mpsc::channel();
        let events = control.clone();
        let source = spec.source();
        let (armed_tx, armed) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("pp-watcher".into())
            .spawn(move || run(source, events, rx, emit, armed_tx))
            .expect("spawn watcher thread");
        // Return once the directories are watched, so no change after `watch` is missed.
        let _ = armed.recv_timeout(CLOSE_TIMEOUT);
        Self { control, thread }
    }

    fn signal(&self) {
        let _ = self.control.send(Msg::Stop);
    }

    /// Joins the thread, which returns once `notify` has closed its handles.
    fn join(self) {
        drop(self.control);
        let _ = self.thread.join();
    }
}

struct Entry {
    spec: WatchSpec,
    running: Running,
}

#[derive(Default)]
struct Inner {
    next_id: WatchId,
    active: HashMap<WatchId, Entry>,
}

/// The app's watchers, keyed by the id each `watch` returns.
pub struct Watchers {
    inner: Mutex<Inner>,
    emit: Emit,
}

impl Watchers {
    pub fn new(emit: Emit) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(Inner::default()),
            emit,
        })
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Starts a watcher and returns its id. Missing paths are watched once they appear.
    pub fn watch(&self, spec: WatchSpec) -> WatchId {
        let id = {
            let mut inner = self.lock();
            inner.next_id += 1;
            inner.next_id
        };
        self.insert(id, spec);
        id
    }

    /// Starts `spec` under `id` (outside the lock: arming can take a moment).
    fn insert(&self, id: WatchId, spec: WatchSpec) {
        let running = Running::start(&spec, self.emit.clone());
        let replaced = self.lock().active.insert(id, Entry { spec, running });
        if let Some(old) = replaced {
            old.running.signal();
            old.running.join();
        }
    }

    /// Stops a watcher and waits for its handles to close; false if `id` is unknown.
    pub fn unwatch(&self, id: WatchId) -> bool {
        let entry = self.lock().active.remove(&id);
        match entry {
            Some(entry) => {
                entry.running.signal();
                entry.running.join();
                true
            }
            None => false,
        }
    }

    /// Stops every watcher at once (the page that asked for them is gone). The threads are
    /// signalled before this returns, so no event reaches a later page from them; the returned
    /// handle waits for their handles to close. Returns how many were stopped.
    pub fn stop_all(&self) -> Stopped {
        let entries: Vec<Entry> = self.lock().active.drain().map(|(_, e)| e).collect();
        for entry in &entries {
            entry.running.signal();
        }
        Stopped(entries)
    }

    /// The ids and specs of the running watchers.
    pub fn active(&self) -> Vec<(WatchId, WatchSpec)> {
        let mut list: Vec<_> = self
            .lock()
            .active
            .iter()
            .map(|(id, entry)| (*id, entry.spec.clone()))
            .collect();
        list.sort_by_key(|(id, _)| *id);
        list
    }

    /// Drops every watcher whose root is at or under `path` and waits until their
    /// handles are closed, so the directory can be deleted (WIN-3.13). The guard
    /// re-creates them under the same ids; dropping it leaves them released.
    pub fn release_under(self: &Arc<Self>, path: &Path) -> ReleaseGuard {
        let released: Vec<(WatchId, Entry)> = {
            let mut inner = self.lock();
            let ids: Vec<WatchId> = inner
                .active
                .iter()
                .filter(|(_, entry)| is_at_or_under(entry.spec.root(), path))
                .map(|(id, _)| *id)
                .collect();
            ids.into_iter()
                .filter_map(|id| inner.active.remove(&id).map(|entry| (id, entry)))
                .collect()
        };
        // Signal every thread first so the handles close in parallel.
        for (_, entry) in &released {
            entry.running.signal();
        }
        let mut specs = Vec::with_capacity(released.len());
        for (id, entry) in released {
            entry.running.join();
            specs.push((id, entry.spec));
        }
        specs.sort_by_key(|(id, _)| *id);
        ReleaseGuard {
            watchers: Arc::downgrade(self),
            specs,
        }
    }
}

impl Drop for Watchers {
    fn drop(&mut self) {
        let inner = self.inner.get_mut().unwrap_or_else(|p| p.into_inner());
        for (_, entry) in inner.active.drain() {
            entry.running.signal();
            entry.running.join();
        }
    }
}

/// Watchers [`Watchers::stop_all`] signalled, still closing their handles.
#[must_use = "join waits for the stopped watchers' handles to close"]
pub struct Stopped(Vec<Entry>);

impl Stopped {
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Waits until every stopped watcher's handles are closed.
    pub fn join(self) {
        for entry in self.0 {
            entry.running.join();
        }
    }
}

/// Watchers released by [`Watchers::release_under`].
#[must_use = "dropping the guard leaves the watchers released"]
pub struct ReleaseGuard {
    watchers: std::sync::Weak<Watchers>,
    specs: Vec<(WatchId, WatchSpec)>,
}

impl ReleaseGuard {
    pub fn specs(&self) -> &[(WatchId, WatchSpec)] {
        &self.specs
    }

    /// Re-creates the released watchers (the request that needed the release failed).
    /// Returns how many were re-created.
    pub fn restore(self) -> usize {
        let Some(watchers) = self.watchers.upgrade() else {
            return 0;
        };
        let mut restored = 0;
        for (id, spec) in self.specs {
            if !watchers.lock().active.contains_key(&id) {
                watchers.insert(id, spec);
                restored += 1;
            }
        }
        restored
    }
}

/// The watcher thread: owns the `notify` watcher, debounces, emits, re-arms.
fn run(
    source: Box<dyn Source>,
    events: Sender<Msg>,
    rx: mpsc::Receiver<Msg>,
    emit: Emit,
    armed: Sender<()>,
) {
    let mut targets = source.targets();
    let mut watcher = arm(&targets, &events);
    let _ = armed.send(());
    drop(armed);
    'outer: loop {
        let first = if watcher.is_some() {
            rx.recv().map_err(|_| RecvTimeoutError::Disconnected)
        } else {
            rx.recv_timeout(RETRY_INTERVAL)
        };
        let mut paths = Vec::new();
        match first {
            Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => break,
            Ok(Msg::Fs(result)) => collect(source.as_ref(), result, &mut paths),
            Err(RecvTimeoutError::Timeout) => {}
        }
        if paths.is_empty() {
            if watcher.is_none() {
                rearm(source.as_ref(), &mut targets, &mut watcher, &events);
            }
            continue;
        }
        loop {
            match rx.recv_timeout(source.debounce()) {
                Ok(Msg::Fs(result)) => collect(source.as_ref(), result, &mut paths),
                Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => break 'outer,
                Err(RecvTimeoutError::Timeout) => break,
            }
        }
        rearm(source.as_ref(), &mut targets, &mut watcher, &events);
        emit(&source.event(paths));
    }
    // Dropping the watcher only asks notify's server thread to stop; it closes the
    // directory handles and then drops our event handler (a `Sender`). Wait for that
    // disconnect so a release returns with the handles closed.
    drop(watcher);
    drop(events);
    let deadline = Instant::now() + CLOSE_TIMEOUT;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok(_) => continue,
            Err(_) => break,
        }
    }
}

fn collect(source: &dyn Source, result: notify::Result<notify::Event>, paths: &mut Vec<PathBuf>) {
    let Ok(event) = result else { return };
    if matches!(event.kind, EventKind::Access(_)) {
        return;
    }
    for path in event.paths {
        if source.relevant(&path) && !paths.contains(&path) {
            paths.push(path);
        }
    }
}

fn rearm(
    source: &dyn Source,
    targets: &mut Vec<(PathBuf, RecursiveMode)>,
    watcher: &mut Option<RecommendedWatcher>,
    events: &Sender<Msg>,
) {
    let next = source.targets();
    if next != *targets || watcher.is_none() {
        // Release the old handles before taking new ones.
        *watcher = None;
        *watcher = arm(&next, events);
        *targets = next;
    }
}

/// A watcher on every existing target, or `None` if none could be watched.
fn arm(targets: &[(PathBuf, RecursiveMode)], events: &Sender<Msg>) -> Option<RecommendedWatcher> {
    if targets.is_empty() {
        return None;
    }
    let tx = events.clone();
    let mut watcher = notify::recommended_watcher(move |result| {
        let _ = tx.send(Msg::Fs(result));
    })
    .ok()?;
    let mut any = false;
    for (dir, mode) in targets {
        if dir.is_dir() && watcher.watch(dir, *mode).is_ok() {
            any = true;
        }
    }
    any.then_some(watcher)
}

/// A comparable spelling of a path: no `\\?\`, `/` separators, no trailing
/// separator, and case-folded on Windows (WINAPP-014).
pub(crate) fn path_key(path: &Path) -> String {
    let raw = path.to_string_lossy();
    let raw = raw.strip_prefix(r"\\?\").unwrap_or(&raw);
    let mut key = raw.replace('\\', "/");
    while key.len() > 1 && key.ends_with('/') && !key.ends_with(":/") {
        key.pop();
    }
    if cfg!(windows) {
        key = key.to_lowercase();
    }
    key
}

/// Whether `path` is `root` or lies under it, by whole components.
pub(crate) fn is_at_or_under(path: &Path, root: &Path) -> bool {
    let path = path_key(path);
    let root = path_key(root);
    path == root
        || path
            .strip_prefix(&root)
            .is_some_and(|rest| rest.starts_with('/') || root.ends_with('/'))
}

/// A path as the UI sees it: no `\\?\` prefix.
pub(crate) fn display(path: &Path) -> String {
    let raw = path.to_string_lossy();
    raw.strip_prefix(r"\\?\").unwrap_or(&raw).to_string()
}

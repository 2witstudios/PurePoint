//! Port of `Services/WorktreeWatcher.swift`: refreshes a worktree's diff when its git
//! metadata changes — the `.git` marker, the git dir's `HEAD` and `index`, and the
//! common dir's `packed-refs` and `refs/` (heads and remotes).
//!
//! Linked worktrees have a `.git` file (`gitdir: …`) and a `commondir`; both are
//! resolved without assuming `.git` is a directory. Git replaces HEAD, index and refs
//! by rename; Swift re-installs its file sources after every burst, and here the
//! directories are watched and the targets recomputed after each burst. Swift
//! debounces 500 ms; so does this.

use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use notify::RecursiveMode;

use super::{Source, WatchEvent, display, is_at_or_under, path_key};

const DEBOUNCE: Duration = Duration::from_millis(500);

/// Where a worktree's git metadata lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitMetadata {
    /// `<worktree>/.git` (a directory or a `gitdir:` file).
    pub marker: PathBuf,
    /// The worktree's own git dir (HEAD, index).
    pub git_dir: PathBuf,
    /// The shared git dir (packed-refs, refs).
    pub common_dir: PathBuf,
}

impl GitMetadata {
    /// Resolves `.git` and `commondir` like `WorktreeWatcher.metadataPaths`.
    pub fn resolve(worktree_path: &Path) -> Self {
        let marker = worktree_path.join(".git");
        let mut git_dir = marker.clone();
        if let Ok(text) = std::fs::read_to_string(&marker)
            && let Some(value) = text.strip_prefix("gitdir:")
        {
            git_dir = normalize(&worktree_path.join(value.trim()));
        }
        let mut common_dir = git_dir.clone();
        if let Ok(text) = std::fs::read_to_string(git_dir.join("commondir")) {
            common_dir = normalize(&git_dir.join(text.trim()));
        }
        Self {
            marker,
            git_dir,
            common_dir,
        }
    }
}

/// Folds `.` and `..` components lexically (`URL.standardizedFileURL`).
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

pub(crate) struct WorktreeSource {
    worktree_path: PathBuf,
}

impl WorktreeSource {
    pub(crate) fn new(worktree_path: &Path) -> Self {
        Self {
            worktree_path: worktree_path.to_path_buf(),
        }
    }
}

impl Source for WorktreeSource {
    fn debounce(&self) -> Duration {
        DEBOUNCE
    }

    fn targets(&self) -> Vec<(PathBuf, RecursiveMode)> {
        let meta = GitMetadata::resolve(&self.worktree_path);
        let mut targets: Vec<(PathBuf, RecursiveMode)> = Vec::new();
        let mut push = |dir: PathBuf, mode: RecursiveMode| {
            if !targets.iter().any(|(d, _)| path_key(d) == path_key(&dir)) {
                targets.push((dir, mode));
            }
        };
        // The worktree directory itself, for the `.git` marker.
        push(self.worktree_path.clone(), RecursiveMode::NonRecursive);
        push(meta.git_dir.clone(), RecursiveMode::NonRecursive);
        push(meta.common_dir.clone(), RecursiveMode::NonRecursive);
        push(meta.common_dir.join("refs"), RecursiveMode::Recursive);
        targets
    }

    fn relevant(&self, path: &Path) -> bool {
        let meta = GitMetadata::resolve(&self.worktree_path);
        let key = path_key(path);
        let in_dir =
            |dir: &Path, names: &[&str]| names.iter().any(|name| key == path_key(&dir.join(name)));
        key == path_key(&meta.marker)
            || in_dir(&meta.git_dir, &["HEAD", "index"])
            || in_dir(&meta.common_dir, &["packed-refs"])
            || is_at_or_under(path, &meta.common_dir.join("refs"))
    }

    fn event(&self, _paths: Vec<PathBuf>) -> WatchEvent {
        WatchEvent::WorktreesChanged {
            worktree_path: display(&self.worktree_path),
        }
    }
}

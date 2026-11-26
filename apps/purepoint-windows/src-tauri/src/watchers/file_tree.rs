//! Port of `Services/FileTreeWatcher.swift`: refreshes the workspace file tree when
//! entries under its root change.
//!
//! Swift opens one `DispatchSource` per expanded directory; `ReadDirectoryChangesW`
//! watches a whole tree from one handle, so this watches the root recursively and drops
//! paths under the names `FileTreeState` hides (`.git`, `node_modules`, …). The event
//! carries the changed paths (at most [`MAX_PATHS`]) so the UI can refresh only the
//! expanded directories that hold them. Swift debounces 500 ms; so does this.

use std::path::{Path, PathBuf};
use std::time::Duration;

use notify::RecursiveMode;

use super::{Source, WatchEvent, display, is_at_or_under};

const DEBOUNCE: Duration = Duration::from_millis(500);

/// The most paths one `file-tree-changed` event lists; the rest still refresh the tree.
pub const MAX_PATHS: usize = 256;

/// `FileTreeState.hiddenNames`: changes under these never refresh the tree.
pub const HIDDEN_NAMES: &[&str] = &[
    ".git",
    ".DS_Store",
    ".build",
    ".swiftpm",
    "xcuserdata",
    "DerivedData",
    "__pycache__",
    ".tsbuildinfo",
    "node_modules",
];

/// Whether `path` (under `root`) lies inside a hidden entry.
pub fn is_hidden(root: &Path, path: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(root) else {
        return false;
    };
    relative.components().any(|component| {
        let name = component.as_os_str().to_string_lossy();
        HIDDEN_NAMES
            .iter()
            .any(|hidden| hidden.eq_ignore_ascii_case(&name))
    })
}

pub(crate) struct FileTreeSource {
    root: PathBuf,
}

impl FileTreeSource {
    pub(crate) fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }
}

impl Source for FileTreeSource {
    fn debounce(&self) -> Duration {
        DEBOUNCE
    }

    fn targets(&self) -> Vec<(PathBuf, RecursiveMode)> {
        vec![(self.root.clone(), RecursiveMode::Recursive)]
    }

    fn relevant(&self, path: &Path) -> bool {
        is_at_or_under(path, &self.root) && !is_hidden(&self.root, path)
    }

    fn event(&self, paths: Vec<PathBuf>) -> WatchEvent {
        WatchEvent::FileTreeChanged {
            root: display(&self.root),
            paths: paths.iter().take(MAX_PATHS).map(|p| display(p)).collect(),
        }
    }
}

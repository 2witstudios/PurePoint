//! Port of `Services/ManifestWatcher.swift`: one refresh per write burst of a project's
//! `.pu/manifest.json`.
//!
//! The daemon writes the manifest with temp + rename, which replaces the file, so the
//! Swift watcher re-opens the new file after each rename. Here the parent directory is
//! watched instead and events are filtered by file name, so a replace needs no re-open.
//! Swift debounces 50 ms; so does this.

use std::path::{Path, PathBuf};
use std::time::Duration;

use notify::RecursiveMode;

use super::{Source, WatchEvent, display, path_key};

const DEBOUNCE: Duration = Duration::from_millis(50);

/// Watches one file through its parent directory (or, until the parent exists, its
/// grandparent, so the parent's creation re-arms the watcher).
pub(crate) struct SingleFile {
    file_key: String,
    parent: PathBuf,
    parent_key: String,
}

impl SingleFile {
    pub(crate) fn new(file: &Path) -> Self {
        let parent = file.parent().map(Path::to_path_buf).unwrap_or_default();
        Self {
            file_key: path_key(file),
            parent_key: path_key(&parent),
            parent,
        }
    }

    pub(crate) fn targets(&self) -> Vec<(PathBuf, RecursiveMode)> {
        if self.parent.is_dir() {
            return vec![(self.parent.clone(), RecursiveMode::NonRecursive)];
        }
        match self.parent.parent() {
            Some(grandparent) if grandparent.is_dir() => {
                vec![(grandparent.to_path_buf(), RecursiveMode::NonRecursive)]
            }
            _ => Vec::new(),
        }
    }

    pub(crate) fn relevant(&self, path: &Path) -> bool {
        let key = path_key(path);
        key == self.file_key || key == self.parent_key
    }
}

pub(crate) struct ManifestSource {
    project_root: PathBuf,
    file: SingleFile,
}

impl ManifestSource {
    pub(crate) fn new(project_root: &Path) -> Self {
        Self {
            project_root: project_root.to_path_buf(),
            file: SingleFile::new(&pu_core::paths::manifest_path(project_root)),
        }
    }
}

impl Source for ManifestSource {
    fn debounce(&self) -> Duration {
        DEBOUNCE
    }

    fn targets(&self) -> Vec<(PathBuf, RecursiveMode)> {
        self.file.targets()
    }

    fn relevant(&self, path: &Path) -> bool {
        self.file.relevant(path)
    }

    fn event(&self, _paths: Vec<PathBuf>) -> WatchEvent {
        WatchEvent::ManifestChanged {
            project_root: display(&self.project_root),
        }
    }
}

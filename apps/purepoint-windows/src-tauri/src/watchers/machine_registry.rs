//! Port of `Services/MachineRegistryWatcher.swift`: reloads machines when
//! `~/.pu/machines.json` changes.
//!
//! `pu machine add`/`remove` and other app instances replace the registry with a locked
//! atomic rename, so the watcher follows the file by name in its parent directory. Until
//! `~/.pu` exists it watches the home directory, and with neither present it retries
//! every second, as the Swift retry timer does. Swift debounces 100 ms; so does this.

use std::path::{Path, PathBuf};
use std::time::Duration;

use notify::RecursiveMode;

use super::manifest::SingleFile;
use super::{Source, WatchEvent};

const DEBOUNCE: Duration = Duration::from_millis(100);

/// `%USERPROFILE%\.pu\machines.json` (REQ-WINAPP-056).
pub fn default_registry_path() -> std::io::Result<PathBuf> {
    pu_core::paths::machines_registry_path()
}

pub(crate) struct MachineRegistrySource {
    file: SingleFile,
}

impl MachineRegistrySource {
    pub(crate) fn new(registry_path: &Path) -> Self {
        Self {
            file: SingleFile::new(registry_path),
        }
    }
}

impl Source for MachineRegistrySource {
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
        WatchEvent::MachinesChanged {}
    }
}

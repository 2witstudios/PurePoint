//! Registry of machines reachable over SSH (`~/.pu/machines.json`).
//!
//! Shared by the app and the CLI. A machine is a user-chosen name plus an SSH
//! destination; its id (a UUID) names its private directory of forwarded
//! sockets under `~/.pu/machines/<id>/`.

use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::PuError;
use crate::paths;

pub const REGISTRY_VERSION: u32 = 1;

const MAX_NAME_LEN: usize = 64;
const MAX_HOST_LEN: usize = 255;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Machine {
    pub id: String,
    pub name: String,
    pub ssh_host: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineRegistry {
    pub version: u32,
    #[serde(default)]
    pub machines: Vec<Machine>,
}

impl Default for MachineRegistry {
    fn default() -> Self {
        Self {
            version: REGISTRY_VERSION,
            machines: Vec::new(),
        }
    }
}

impl MachineRegistry {
    /// Names match case-insensitively, as `--machine` does.
    pub fn find(&self, name: &str) -> Option<&Machine> {
        self.machines
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case(name))
    }

    pub fn add(&mut self, name: &str, ssh_host: &str) -> Result<Machine, PuError> {
        validate_name(name)?;
        validate_ssh_host(ssh_host)?;
        if let Some(existing) = self.find(name) {
            return Err(PuError::InvalidArgument(format!(
                "a machine named {:?} already exists",
                existing.name
            )));
        }
        let machine = Machine {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            ssh_host: ssh_host.to_string(),
        };
        self.machines.push(machine.clone());
        Ok(machine)
    }

    pub fn remove(&mut self, name: &str) -> Option<Machine> {
        let index = self
            .machines
            .iter()
            .position(|m| m.name.eq_ignore_ascii_case(name))?;
        Some(self.machines.remove(index))
    }

    fn validate(&self) -> Result<(), PuError> {
        if self.version != REGISTRY_VERSION {
            return Err(PuError::InvalidArgument(format!(
                "unsupported machine registry version {} (expected {REGISTRY_VERSION}) — update PurePoint",
                self.version
            )));
        }
        for (i, machine) in self.machines.iter().enumerate() {
            validate_id(&machine.id)?;
            validate_name(&machine.name)?;
            validate_ssh_host(&machine.ssh_host)?;
            if self.machines[..i]
                .iter()
                .any(|m| m.name.eq_ignore_ascii_case(&machine.name))
            {
                return Err(PuError::InvalidArgument(format!(
                    "duplicate machine name {:?} in registry",
                    machine.name
                )));
            }
        }
        Ok(())
    }
}

/// Names are what `--machine` matches: `[A-Za-z0-9._-]`, starting alphanumeric.
pub fn validate_name(name: &str) -> Result<(), PuError> {
    let valid = !name.is_empty()
        && name.len() <= MAX_NAME_LEN
        && name.starts_with(|c: char| c.is_ascii_alphanumeric())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if valid {
        Ok(())
    } else {
        Err(PuError::InvalidArgument(format!(
            "invalid machine name {name:?} — use 1-{MAX_NAME_LEN} letters, digits, '.', '_' or '-', starting with a letter or digit"
        )))
    }
}

/// An SSH destination: a `~/.ssh/config` alias or `user@host[:port]`. It is
/// passed to ssh as one argv element, so it must not look like an option.
pub fn validate_ssh_host(host: &str) -> Result<(), PuError> {
    let valid = !host.is_empty()
        && host.len() <= MAX_HOST_LEN
        && !host.starts_with('-')
        && !host.chars().any(|c| c.is_whitespace() || c.is_control());
    if valid {
        Ok(())
    } else {
        Err(PuError::InvalidArgument(format!(
            "invalid SSH destination {host:?} — use an ~/.ssh/config alias or user@host[:port]"
        )))
    }
}

/// Ids name directories, so only UUIDs are accepted.
pub fn validate_id(id: &str) -> Result<(), PuError> {
    uuid::Uuid::parse_str(id)
        .map(|_| ())
        .map_err(|_| PuError::InvalidArgument(format!("invalid machine id {id:?}")))
}

/// Load the registry; a missing file is an empty registry.
pub fn load(path: &Path) -> Result<MachineRegistry, PuError> {
    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(MachineRegistry::default());
        }
        Err(e) => return Err(PuError::Io(e)),
    };
    let registry: MachineRegistry = serde_json::from_str(&content)?;
    registry.validate()?;
    Ok(registry)
}

/// Atomically replace the registry with a file only the user can read.
/// Private: callers go through [`update`], which holds the registry lock.
fn save(path: &Path, registry: &MachineRegistry) -> Result<(), PuError> {
    registry.validate()?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let content = serde_json::to_string_pretty(registry)? + "\n";
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "machines.json".into());
    let temp = parent.join(format!(".{file_name}.tmp.{}", uuid::Uuid::new_v4()));
    let write = || -> std::io::Result<()> {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)?;
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&temp, path)?;
        std::fs::File::open(parent)?.sync_all()
    };
    write().inspect_err(|_| {
        let _ = std::fs::remove_file(&temp);
    })?;
    Ok(())
}

/// Load, change and save the registry under an exclusive lock, so the app
/// and concurrent CLI invocations never lose each other's edits.
pub fn update<T>(
    path: &Path,
    change: impl FnOnce(&mut MachineRegistry) -> Result<T, PuError>,
) -> Result<T, PuError> {
    use fs4::FileExt;
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let mut lock_path = path.as_os_str().to_os_string();
    lock_path.push(".lock");
    // Never deleted: unlinking a lock someone waits on breaks mutual exclusion.
    let lock_file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o600)
        .open(PathBuf::from(lock_path))?;
    FileExt::lock(&lock_file)?;
    let mut registry = load(path)?;
    let result = change(&mut registry)?;
    save(path, &registry)?;
    Ok(result)
}

/// Create `dir` (and missing parents) and force it to mode 0700.
pub fn ensure_private_dir(dir: &Path) -> Result<(), PuError> {
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    Ok(())
}

/// Create `~/.pu/machines/<id>` (both levels 0700) and return it.
pub fn ensure_machine_dir(machine_id: &str) -> Result<PathBuf, PuError> {
    validate_id(machine_id)?;
    ensure_private_dir(&paths::machines_dir()?)?;
    let dir = paths::machine_dir(machine_id)?;
    ensure_private_dir(&dir)?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mode(path: &Path) -> u32 {
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn given_missing_file_should_load_empty_registry() {
        let tmp = tempfile::TempDir::new().unwrap();
        let registry = load(&tmp.path().join("machines.json")).unwrap();
        assert_eq!(registry, MachineRegistry::default());
        assert_eq!(registry.version, 1);
    }

    #[test]
    fn given_added_machine_should_round_trip_through_private_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("nested").join("machines.json");
        let added = update(&path, |r| r.add("studio", "studio-mac")).unwrap();
        assert!(uuid::Uuid::parse_str(&added.id).is_ok());
        assert_eq!(mode(&path), 0o600);

        let registry = load(&path).unwrap();
        assert_eq!(registry.machines, vec![added.clone()]);
        let raw: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(raw["version"], 1);
        assert_eq!(raw["machines"][0]["name"], "studio");
        assert_eq!(raw["machines"][0]["ssh_host"], "studio-mac");
        assert_eq!(raw["machines"][0]["id"], added.id.as_str());
        // No temp files are left behind.
        let leftovers: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains(".tmp."))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }

    #[test]
    fn given_duplicate_name_in_any_case_should_reject_add() {
        let mut registry = MachineRegistry::default();
        registry.add("Studio", "a").unwrap();
        let err = registry.add("studio", "b").unwrap_err().to_string();
        assert!(err.contains("already exists"), "{err}");
        assert_eq!(registry.machines.len(), 1);
    }

    #[test]
    fn given_name_in_other_case_should_find_and_remove() {
        let mut registry = MachineRegistry::default();
        registry.add("Studio", "a").unwrap();
        assert_eq!(registry.find("STUDIO").unwrap().ssh_host, "a");
        assert!(registry.find("other").is_none());
        assert_eq!(registry.remove("studio").unwrap().name, "Studio");
        assert!(registry.remove("studio").is_none());
        assert!(registry.machines.is_empty());
    }

    #[test]
    fn given_invalid_names_should_reject() {
        for name in [
            "",
            "-x",
            ".hidden",
            "has space",
            "slash/name",
            "colon:x",
            &"a".repeat(65),
        ] {
            assert!(validate_name(name).is_err(), "{name:?}");
        }
        for name in ["studio", "Mac-2", "build_box.local", "1"] {
            assert!(validate_name(name).is_ok(), "{name:?}");
        }
    }

    #[test]
    fn given_option_like_or_blank_host_should_reject() {
        for host in ["", "-oProxyCommand=x", "a b", "a\nb"] {
            assert!(validate_ssh_host(host).is_err(), "{host:?}");
        }
        for host in ["studio-mac", "me@10.0.0.2", "me@host:2222"] {
            assert!(validate_ssh_host(host).is_ok(), "{host:?}");
        }
    }

    #[test]
    fn given_registry_with_unsafe_id_or_newer_version_should_refuse_to_load() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("machines.json");
        std::fs::write(
            &path,
            r#"{"version":1,"machines":[{"id":"../../etc","name":"x","ssh_host":"h"}]}"#,
        )
        .unwrap();
        assert!(load(&path).unwrap_err().to_string().contains("machine id"));
        std::fs::write(&path, r#"{"version":2,"machines":[]}"#).unwrap();
        assert!(load(&path).unwrap_err().to_string().contains("version"));
    }

    #[test]
    fn given_removed_machine_should_persist_removal() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("machines.json");
        update(&path, |r| r.add("a", "ha")).unwrap();
        update(&path, |r| r.add("b", "hb")).unwrap();
        let removed = update(&path, |r| Ok(r.remove("A"))).unwrap();
        assert_eq!(removed.unwrap().ssh_host, "ha");
        let names: Vec<_> = load(&path)
            .unwrap()
            .machines
            .into_iter()
            .map(|m| m.name)
            .collect();
        assert_eq!(names, vec!["b"]);
    }

    #[test]
    fn given_failed_change_should_leave_registry_untouched() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("machines.json");
        update(&path, |r| r.add("a", "ha")).unwrap();
        let before = std::fs::read_to_string(&path).unwrap();
        assert!(update(&path, |r| r.add("A", "other")).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
    }

    #[test]
    fn given_machine_id_should_create_private_dirs() {
        let tmp = tempfile::TempDir::new().unwrap();
        paths::set_home_override(Some(tmp.path().to_path_buf()));
        let machines = tmp.path().join(".pu").join("machines");
        std::fs::create_dir_all(&machines).unwrap();
        std::fs::set_permissions(&machines, std::fs::Permissions::from_mode(0o755)).unwrap();
        let id = uuid::Uuid::new_v4().to_string();

        let dir = ensure_machine_dir(&id).unwrap();

        assert_eq!(dir, machines.join(&id));
        assert_eq!(mode(&machines), 0o700);
        assert_eq!(mode(&dir), 0o700);
        assert!(ensure_machine_dir("../escape").is_err());
        paths::set_home_override(None);
    }
}

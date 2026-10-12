//! Receiver-owned immutable project exports. Remote requests supply opaque IDs,
//! never filesystem paths. Retargeting or re-enabling requires a new export ID
//! and explicit new peer grants; existing IDs and revocation tombstones persist.
//!
//! Place this store inside the same authority barrier as MeshStore and Ledger.
//! Local administration takes exclusive authority; dispatch holds shared authority
//! through exact lookup and effects. The private file lock also serializes separate
//! instances using the same daemon lock. This module does not authorize peers,
//! prove source readiness, create worktrees or contain same-account code.

use std::ffi::OsString;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use nix::fcntl::{Flock, FlockArg};
use pu_core::mesh::valid_identifier;
use pu_core::private_fs::Entry;
use serde::{Deserialize, Serialize};

use super::DaemonLock;
use crate::execution_state::LocalAdmin;

const MAX_RECORD_BYTES: u64 = 16 * 1024;
const MAX_EXPORTS: usize = 1024;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("private export state I/O failed")]
    Io(#[from] std::io::Error),
    #[error("invalid export state; explicit local recovery required")]
    Invalid,
    #[error("export is unavailable, revoked or has changed")]
    Unavailable,
    #[error("export record limit reached; retained IDs cannot be recycled")]
    Capacity,
}
pub type Result<T> = std::result::Result<T, Error>;

/// Local administrative description. Never serialize this directly onto the
/// bridge: the project path is receiver-private and inventory is grant-filtered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportRecord {
    pub id: String,
    pub generation: u64,
    pub label: String,
    pub revoked: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    schema: u32,
    owner_daemon: String,
    id: String,
    label: String,
    canonical_root: PathBuf,
    device: u64,
    inode: u64,
}

/// Local lookup evidence, not an authorization capability. Keep the authority
/// and per-agent guards, and revalidate before subsequent filesystem effects.
/// Inode/path checks protect against accidental replacement, not malicious
/// same-account modifications or restored/rolled-back authority data.
#[derive(Clone, Debug)]
pub struct ResolvedExport(Binding);

impl ResolvedExport {
    pub fn id(&self) -> &str {
        &self.0.id
    }
    pub fn generation(&self) -> u64 {
        1
    }
    pub fn project_root(&self) -> &Path {
        &self.0.canonical_root
    }
}

/// Bound on waiting for another catalog transaction to finish.
const LOCK_WAIT: std::time::Duration = std::time::Duration::from_secs(2);

pub struct MeshExports {
    lock: Arc<DaemonLock>,
    owner_daemon: String,
    limit: usize,
}

impl MeshExports {
    /// Missing individual exports mean no access. Existing malformed records
    /// fail closed; this never reconstructs or replaces an export catalog.
    /// `owner_daemon` must come from the persisted receiver MeshStore identity.
    pub fn open(lock: Arc<DaemonLock>, owner_daemon: &str, limit: usize) -> Result<Self> {
        if !valid_identifier(owner_daemon) || limit == 0 || limit > MAX_EXPORTS {
            return Err(Error::Invalid);
        }
        let exports = Self {
            lock,
            owner_daemon: owner_daemon.into(),
            limit,
        };
        let _transaction = exports.transaction()?;
        exports.records()?;
        Ok(exports)
    }

    pub fn owner_daemon(&self) -> &str {
        &self.owner_daemon
    }

    fn name(&self, suffix: &str) -> OsString {
        let mut name = self.lock.socket_name.clone();
        name.push(suffix);
        name
    }

    fn record_name(&self, id: &str) -> Result<OsString> {
        if !valid_identifier(id) {
            return Err(Error::Invalid);
        }
        Ok(self.name(&format!(".mesh-export-{id}.json")))
    }

    fn revoked_name(&self, id: &str) -> Result<OsString> {
        if !valid_identifier(id) {
            return Err(Error::Invalid);
        }
        Ok(self.name(&format!(".mesh-export-{id}.revoked")))
    }

    // Nonblocking: this synchronous API never waits indefinitely for another
    // operation. Callers may explicitly retry local administration on contention.
    /// Exclusive catalog access. Concurrent sessions resolve exports under the
    /// shared authority barrier, so contention is normal: wait briefly for the
    /// holder instead of failing, but never block indefinitely on a stuck one.
    fn transaction(&self) -> Result<Flock<File>> {
        self.lock.verify_lock()?;
        let name = self.name(".mesh-exports.lock");
        let deadline = std::time::Instant::now() + LOCK_WAIT;
        let mut file = self.lock.directory.open_file(&name, true)?;
        let guard = loop {
            match Flock::lock(file, FlockArg::LockExclusiveNonblock) {
                Ok(guard) => break guard,
                Err((returned, nix::errno::Errno::EWOULDBLOCK))
                    if std::time::Instant::now() < deadline =>
                {
                    file = returned;
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
                Err((_, error)) => return Err(std::io::Error::from(error).into()),
            }
        };
        self.lock
            .directory
            .require_identity(&name, Entry::from_file(&guard)?)?
            .private_file()?;
        Ok(guard)
    }

    fn read_binding(&self, id: &str) -> Result<Binding> {
        let name = self.record_name(id)?;
        let file = self.lock.directory.open_file(&name, false)?;
        if file.metadata()?.len() > MAX_RECORD_BYTES {
            return Err(Error::Invalid);
        }
        let mut bytes = Vec::new();
        file.take(MAX_RECORD_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_RECORD_BYTES {
            return Err(Error::Invalid);
        }
        let record: Binding = serde_json::from_slice(&bytes).map_err(|_| Error::Invalid)?;
        if record.schema != 1
            || record.owner_daemon != self.owner_daemon
            || record.id != id
            || !valid_identifier(&record.id)
            || !valid_label(&record.label)
            || !valid_root(&record.canonical_root)
            || record.inode == 0
        {
            return Err(Error::Invalid);
        }
        self.lock.verify_lock()?;
        Ok(record)
    }

    fn is_revoked(&self, id: &str) -> Result<bool> {
        // Any entry is a fence, including an empty/torn/unsafe tombstone. Never
        // follow or repair it in order to turn revocation into access.
        Ok(self
            .lock
            .directory
            .entry(&self.revoked_name(id)?)?
            .is_some())
    }

    fn records(&self) -> Result<Vec<ExportRecord>> {
        let anchor = self
            .lock
            .directory
            .checked_path_for(&self.lock.socket_name)?;
        let directory = anchor.parent().ok_or(Error::Invalid)?;
        let prefix = self.name(".mesh-export-");
        let prefix = prefix.to_str().ok_or(Error::Invalid)?;
        let mut result = Vec::new();
        // Scan is over private daemon metadata. Do not retain unrelated names;
        // matching records, including revoked records, have an explicit cap.
        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            let Some(id) = name
                .strip_prefix(prefix)
                .and_then(|s| s.strip_suffix(".json"))
            else {
                continue;
            };
            if result.len() >= self.limit {
                return Err(Error::Capacity);
            }
            let binding = self.read_binding(id)?;
            result.push(ExportRecord {
                id: binding.id,
                generation: 1,
                label: binding.label,
                revoked: self.is_revoked(id)?,
            });
        }
        self.lock.verify_lock()?;
        result.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(result)
    }

    pub fn list(&self, _admin: &LocalAdmin) -> Result<Vec<ExportRecord>> {
        let _transaction = self.transaction()?;
        self.records()
    }

    fn create(&self, name: &std::ffi::OsStr, bytes: &[u8]) -> Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
            .open(self.lock.directory.checked_path_for(name)?)?;
        let expected = Entry::from_file(&file)?;
        expected.private_file()?;
        // Shared utility additionally rejects ACL grants on the newly created
        // entry before any project metadata is written.
        let checked = self.lock.directory.open_file(name, false)?;
        if !expected.same_file(Entry::from_file(&checked)?) {
            return Err(Error::Invalid);
        }
        file.write_all(bytes)?;
        file.sync_all()?;
        self.lock.directory.sync()?;
        self.lock
            .directory
            .require_identity(name, expected)?
            .private_file()?;
        self.lock.verify_lock()?;
        Ok(())
    }

    /// Register a local path once. Alias inputs are canonicalized at explicit
    /// enrollment; later lookup refuses any change of that canonical binding.
    pub fn register(
        &mut self,
        _admin: &LocalAdmin,
        project: &Path,
        label: &str,
    ) -> Result<ExportRecord> {
        let _transaction = self.transaction()?;
        if !valid_label(label) {
            return Err(Error::Invalid);
        }
        if self.records()?.len() >= self.limit {
            return Err(Error::Capacity);
        }
        let root = project.canonicalize()?;
        if !valid_root(&root) {
            return Err(Error::Invalid);
        }
        let metadata = std::fs::symlink_metadata(&root)?;
        if !metadata.is_dir() {
            return Err(Error::Invalid);
        }
        let binding = Binding {
            schema: 1,
            owner_daemon: self.owner_daemon.clone(),
            id: pu_core::id::session_id(),
            label: label.into(),
            canonical_root: root,
            device: metadata.dev(),
            inode: metadata.ino(),
        };
        let name = self.record_name(&binding.id)?;
        // Includes a cryptographic UUID and O_EXCL. Even a collision with an
        // old tombstone must never reuse an identity whose record was lost.
        if self.is_revoked(&binding.id)? {
            return Err(Error::Unavailable);
        }
        self.create(
            &name,
            &serde_json::to_vec(&binding).map_err(|_| Error::Invalid)?,
        )?;
        verify_directory(&binding)?;
        Ok(ExportRecord {
            id: binding.id,
            generation: 1,
            label: binding.label,
            revoked: false,
        })
    }

    /// Irreversible for this ID. Revoke works even when the project is absent or
    /// its record is damaged; the caller still needs an explicit local capability.
    pub fn revoke(&mut self, _admin: &LocalAdmin, id: &str) -> Result<()> {
        let _transaction = self.transaction()?;
        let name = self.revoked_name(id)?;
        if self.is_revoked(id)? {
            // An earlier attempt may have created the fence but failed to sync.
            // Idempotent success must establish durability too. Unsafe entries
            // remain fences, but cannot be reported as a verified durable write.
            self.lock.directory.open_file(&name, true)?.sync_all()?;
            self.lock.directory.sync()?;
            self.lock.verify_lock()?;
            return Ok(());
        }
        if self.lock.directory.entry(&self.record_name(id)?)?.is_none() {
            return Err(Error::Unavailable);
        }
        self.create(&name, b"revoked\n")
    }

    pub fn resolve(&self, id: &str, generation: u64) -> Result<ResolvedExport> {
        let _transaction = self.transaction()?;
        if generation != 1 || self.is_revoked(id)? {
            return Err(Error::Unavailable);
        }
        let binding = self.read_binding(id)?;
        verify_directory(&binding)?;
        Ok(ResolvedExport(binding))
    }

    pub fn revalidate(&self, resolved: &ResolvedExport) -> Result<()> {
        let current = self.resolve(resolved.id(), resolved.generation())?;
        if current.0 != resolved.0 {
            return Err(Error::Unavailable);
        }
        Ok(())
    }
}

fn valid_label(label: &str) -> bool {
    !label.trim().is_empty() && label.len() <= 128 && !label.chars().any(char::is_control)
}

fn valid_root(path: &Path) -> bool {
    path.is_absolute()
        && path
            .to_str()
            .is_some_and(|s| s.len() <= 4096 && !s.chars().any(char::is_control))
        && path.components().all(|c| {
            matches!(
                c,
                std::path::Component::RootDir | std::path::Component::Normal(_)
            )
        })
}

fn verify_directory(binding: &Binding) -> Result<()> {
    if binding.canonical_root.canonicalize()? != binding.canonical_root {
        return Err(Error::Unavailable);
    }
    let metadata = std::fs::symlink_metadata(&binding.canonical_root)?;
    if !metadata.is_dir() || metadata.dev() != binding.device || metadata.ino() != binding.inode {
        return Err(Error::Unavailable);
    }
    Ok(())
}

#[cfg(test)]
mod tests;

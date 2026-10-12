use super::*;
use pu_core::private_fs::Entry;
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::io::{Read, Write};
use std::os::unix::ffi::OsStrExt;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ConfigRecord {
    pub schema: u32,
    pub owner_daemon: String,
    pub revision: u64,
    pub config: ListenConfig,
    /// Host-side routing hints; absent in records written before worker routing.
    #[serde(default)]
    pub workers: Vec<WorkerRoute>,
}

impl ConfigRecord {
    pub(super) fn valid_workers(&self) -> bool {
        let mut seen = std::collections::BTreeSet::new();
        self.workers.len() <= MAX_WORKER_ROUTES
            && self
                .workers
                .iter()
                .all(|route| route.validate() && seen.insert(route.fingerprint.as_str()))
    }
}

/// Bounds the private configuration record, including every worker route.
const CONFIG_LIMIT: u64 = 64 * 1024;

pub(super) fn name(lock: &DaemonLock, suffix: &str) -> OsString {
    let mut name = lock.socket_name.clone();
    name.push(suffix);
    name
}

pub(super) fn require_file(lock: &DaemonLock, suffix: &str) -> Result<()> {
    lock.verify_lock()
        .map_err(|_| Error::State("private directory or singleton lock"))?;
    lock.directory
        .entry(&name(lock, suffix))
        .map_err(|_| Error::State("private mesh entry"))?
        .ok_or(Error::State(
            "missing existing mesh entry; explicit recovery required",
        ))?
        .private_file()
        .map_err(|_| Error::State("unsafe existing mesh entry"))?;
    Ok(())
}

pub(super) fn require_fresh(lock: &DaemonLock) -> Result<()> {
    lock.verify_lock()
        .map_err(|_| Error::State("private directory or singleton lock"))?;
    let path = lock
        .directory
        .checked_path_for(&lock.socket_name)
        .map_err(|_| Error::State("private directory"))?;
    let prefix = name(lock, ".mesh-");
    // Detect retained mesh metadata before explicitly creating a new identity. Peer
    // enumeration itself uses the bounded ledger API, never this directory scan.
    let entries = std::fs::read_dir(path.parent().ok_or(Error::State("private directory"))?)
        .map_err(|_| Error::State("private directory"))?;
    for (index, entry) in entries.enumerate() {
        if index >= 4096 {
            return Err(Error::State("private directory entry budget"));
        }
        let entry = entry.map_err(|_| Error::State("private directory"))?;
        if entry.file_name().as_bytes().starts_with(prefix.as_bytes()) {
            return Err(Error::AlreadyInitialized);
        }
    }
    lock.verify_lock()
        .map_err(|_| Error::State("private directory or singleton lock"))?;
    Ok(())
}

pub(super) fn read_config(lock: &DaemonLock, owner: &str) -> Result<ConfigRecord> {
    require_file(lock, ".mesh-listen.json")?;
    let file = lock
        .directory
        .open_file(&name(lock, ".mesh-listen.json"), false)
        .map_err(|_| Error::State("listener configuration"))?;
    if file
        .metadata()
        .map_err(|_| Error::State("listener configuration"))?
        .len()
        > CONFIG_LIMIT
    {
        return Err(Error::State("listener configuration size"));
    }
    let mut bytes = Vec::new();
    file.take(CONFIG_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::State("listener configuration"))?;
    if bytes.len() as u64 > CONFIG_LIMIT {
        return Err(Error::State("listener configuration size"));
    }
    let record: ConfigRecord = serde_json::from_slice(&bytes)
        .map_err(|_| Error::State("malformed listener configuration"))?;
    if record.schema != 1
        || record.owner_daemon != owner
        || record.revision == 0
        || !record.config.validate()
        || !record.valid_workers()
    {
        return Err(Error::State(
            "listener configuration owner, revision or interface",
        ));
    }
    lock.verify_lock()
        .map_err(|_| Error::State("private directory or singleton lock"))?;
    Ok(record)
}

pub(super) fn write_config(
    lock: &DaemonLock,
    record: &ConfigRecord,
    initialize: bool,
) -> Result<()> {
    lock.verify_lock()
        .map_err(|_| Error::State("private directory or singleton lock"))?;
    let destination = name(lock, ".mesh-listen.json");
    let original_entry = lock
        .directory
        .entry(&destination)
        .map_err(|_| Error::State("listener configuration"))?;
    match original_entry {
        Some(entry) => {
            if initialize {
                return Err(Error::AlreadyInitialized);
            }
            entry
                .private_file()
                .map_err(|_| Error::State("unsafe listener configuration"))?;
        }
        None if !initialize => return Err(Error::State("missing listener configuration")),
        None => {}
    }
    let bytes =
        serde_json::to_vec(record).map_err(|_| Error::State("listener configuration encoding"))?;
    let temporary = name(
        lock,
        &format!(".mesh-listen-{}.tmp", pu_core::id::session_id()),
    );
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(
            lock.directory
                .checked_path_for(&temporary)
                .map_err(|_| Error::State("private directory"))?,
        )
        .map_err(|_| Error::State("listener configuration temporary"))?;
    let result = (|| {
        file.write_all(&bytes)
            .map_err(|_| Error::State("listener configuration write"))?;
        file.sync_all()
            .map_err(|_| Error::State("listener configuration sync"))?;
        lock.verify_lock()
            .map_err(|_| Error::State("private directory or singleton lock"))?;
        lock.directory
            .require_identity(
                &temporary,
                Entry::from_file(&file)
                    .map_err(|_| Error::State("listener configuration temporary"))?,
            )
            .map_err(|_| Error::State("listener configuration temporary"))?
            .private_file()
            .map_err(|_| Error::State("listener configuration temporary"))?;
        // Publication occurs only within the revalidated private directory under
        // the retained daemon singleton; same-UID hostile races are not a sandbox.
        let current_entry = lock
            .directory
            .entry(&destination)
            .map_err(|_| Error::State("listener configuration"))?;
        match (original_entry, current_entry) {
            (None, None) => {}
            (Some(before), Some(after)) if before.same_file(after) => {
                after
                    .private_file()
                    .map_err(|_| Error::State("unsafe listener configuration"))?;
            }
            _ => {
                return Err(Error::State(
                    "listener configuration replaced during publication",
                ));
            }
        }
        std::fs::rename(
            lock.directory
                .checked_path_for(&temporary)
                .map_err(|_| Error::State("private directory"))?,
            lock.directory
                .checked_path_for(&destination)
                .map_err(|_| Error::State("private directory"))?,
        )
        .map_err(|_| Error::State("listener configuration publication"))?;
        lock.directory
            .sync()
            .map_err(|_| Error::State("listener configuration directory sync"))?;
        lock.verify_lock()
            .map_err(|_| Error::State("private directory or singleton lock"))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = lock.directory.remove_file(&temporary, &file);
    }
    result
}

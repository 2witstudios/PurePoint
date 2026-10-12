use std::ffi::OsString;
use std::fs::File;
use std::io::{self, Read, Seek, Write};
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use nix::errno::Errno;
use nix::fcntl::{Flock, FlockArg};
use nix::sys::signal;
use nix::unistd::Pid;
use tokio::net::{UnixListener, UnixSocket};

pub mod mesh_store;
pub mod owned_work;
mod private_state;
use private_state::{Entry, PrivateDir, denied};

/// Owns the private directory, single-writer lock and exact runtime artifacts.
/// The lock file is never unlinked: doing so would permit two lock holders.
/// Keep this guard alive until the listener and all shutdown work have ended.
pub struct DaemonLock {
    directory: PrivateDir,
    socket_name: OsString,
    lock_name: OsString,
    pid_name: OsString,
    lock: Flock<File>,
    pid: Option<File>,
    socket: Option<Entry>,
    active_work: AtomicUsize,
}

/// Lock file guarding the full socket name.
pub fn lock_path_for(socket_path: &Path) -> PathBuf {
    sibling_path(socket_path, "lock")
}

/// PID file for the daemon on this socket.
pub fn pid_path_for(socket_path: &Path) -> PathBuf {
    sibling_path(socket_path, "pid")
}

fn sibling_path(socket_path: &Path, ext: &str) -> PathBuf {
    let mut name = socket_path.as_os_str().to_owned();
    name.push(".");
    name.push(ext);
    PathBuf::from(name)
}

/// Validate/create a private socket directory and acquire its lifetime lock.
/// Returns `None` for contention; unsafe entries are errors and left untouched.
pub fn try_acquire_daemon_lock(socket_path: &Path) -> io::Result<Option<DaemonLock>> {
    let name = socket_path
        .file_name()
        .ok_or_else(|| denied("socket needs a file name"))?;
    let directory = PrivateDir::open(socket_path.parent().unwrap_or(Path::new(".")), true)?;
    let lock_name = sibling_path(Path::new(name), "lock").into_os_string();
    let file = directory.open_file(&lock_name, true)?;
    let lock = match Flock::lock(file, FlockArg::LockExclusiveNonblock) {
        Ok(lock) => lock,
        Err((_, Errno::EWOULDBLOCK)) => return Ok(None),
        Err((_, e)) => return Err(e.into()),
    };
    let guard = DaemonLock {
        directory,
        socket_name: name.to_owned(),
        lock_name,
        pid_name: sibling_path(Path::new(name), "pid").into_os_string(),
        lock,
        pid: None,
        socket: None,
        active_work: AtomicUsize::new(0),
    };
    guard.verify_lock()?;
    guard.lock.sync_all()?;
    guard.directory.sync()?;
    Ok(Some(guard))
}

impl DaemonLock {
    fn verify_lock(&self) -> io::Result<()> {
        self.directory.verify()?;
        self.directory
            .require_identity(&self.lock_name, Entry::from_file(&self.lock)?)?
            .private_file()
    }

    /// Durably record the advisory PID while holding the authoritative flock.
    /// Write through the validated descriptor, never a predictable temp path.
    /// Readers may briefly see an empty PID; single-writer ownership uses flock.
    pub fn claim_pid_file(&mut self) -> io::Result<()> {
        // Refuse a live or unsafe socket before overwriting even advisory
        // metadata belonging to a non-cooperating/older listener.
        self.stale_socket()?;
        let file = self.directory.open_file(&self.pid_name, true)?;
        self.pid = Some(file);
        let file = self.pid.as_mut().expect("PID descriptor just assigned");
        file.set_len(0)?;
        file.rewind()?;
        writeln!(file, "{}", std::process::id())?;
        file.sync_all()?;
        self.directory.sync()?;
        self.verify_lock()?;
        self.directory
            .require_identity(
                &self.pid_name,
                Entry::from_file(self.pid.as_ref().unwrap())?,
            )?
            .private_file()
    }

    fn stale_socket(&self) -> io::Result<Option<Entry>> {
        self.verify_lock()?;
        let existing = self.directory.entry(&self.socket_name)?;
        if let Some(entry) = existing {
            entry.owned_socket()?;
            // A lock alone does not prove staleness: an older/embedded server
            // may have a live listener without participating in our flock.
            if !socket_is_stale(&self.directory.path_for(&self.socket_name))? {
                return Err(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    "existing socket is live",
                ));
            }
        }
        Ok(existing)
    }

    pub(crate) fn bind_socket(&mut self) -> io::Result<UnixListener> {
        if let Some(entry) = self.stale_socket()? {
            self.verify_lock()?;
            self.directory.remove_socket(&self.socket_name, entry)?;
        }
        let path = self.directory.path_for(&self.socket_name);
        let socket = UnixSocket::new_stream()?;
        socket.bind(&path)?;
        let entry = self
            .directory
            .entry(&self.socket_name)?
            .ok_or_else(|| denied("new socket disappeared"))?;
        entry.owned_socket()?;
        self.socket = Some(entry);
        self.directory.protect_socket(&self.socket_name, entry)?;
        self.verify_lock()?;
        // No accept/listen until private mode has been established.
        socket.listen(1024)
    }

    /// Remove only artifacts still matching the descriptors/identity we own.
    /// Safe to call explicitly before process::exit and again from Drop.
    pub fn cleanup(&self) {
        if self.active_work.load(Ordering::Acquire) != 0 {
            tracing::debug!("retaining lifecycle artifacts while accepted work is pending");
            return;
        }
        if let Err(error) = self.verify_lock() {
            tracing::warn!(%error, "preserving lifecycle files: ownership validation failed");
            return;
        }
        if let Some(socket) = self.socket
            && !matches!(self.directory.entry(&self.socket_name), Ok(None))
            && let Err(error) = self.directory.remove_socket(&self.socket_name, socket)
        {
            tracing::warn!(%error, "preserving replaced or unsafe socket during cleanup");
        }
        if let Some(pid) = &self.pid
            && !matches!(self.directory.entry(&self.pid_name), Ok(None))
            && let Err(error) = self.directory.remove_file(&self.pid_name, pid)
        {
            tracing::warn!(%error, "preserving replaced or unsafe PID file during cleanup");
        }
    }
}

impl Drop for DaemonLock {
    fn drop(&mut self) {
        self.cleanup();
    }
}

/// Read only an owned, private, single-link regular PID file without following links.
pub fn read_pid_file(path: &Path) -> io::Result<Option<u32>> {
    let read = || -> io::Result<Option<u32>> {
        let directory = PrivateDir::open(path.parent().unwrap_or(Path::new(".")), false)?;
        let name = path
            .file_name()
            .ok_or_else(|| denied("PID needs a file name"))?;
        let file = directory.open_file(name, false)?;
        let mut content = String::new();
        file.take(32).read_to_string(&mut content)?;
        Ok(content.trim().parse().ok())
    };
    match read() {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        result => result,
    }
}

fn socket_is_stale(path: &Path) -> io::Result<bool> {
    let socket = UnixSocket::new_stream()?;
    let bytes = path.as_os_str().as_bytes();
    // SAFETY: zero is a valid initial representation of sockaddr_un.
    let mut addr: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    if bytes.contains(&0) || bytes.len() >= addr.sun_path.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid Unix socket path",
        ));
    }
    addr.sun_family = libc::AF_UNIX as _;
    for (dst, src) in addr.sun_path.iter_mut().zip(bytes) {
        *dst = *src as _;
    }
    let len = std::mem::offset_of!(libc::sockaddr_un, sun_path) + bytes.len() + 1;
    #[cfg(target_os = "macos")]
    {
        addr.sun_len = len as _;
    }
    // SAFETY: valid nonblocking socket and initialized sockaddr of `len` bytes.
    let result = unsafe {
        libc::connect(
            socket.as_raw_fd(),
            (&addr as *const libc::sockaddr_un).cast(),
            len as _,
        )
    };
    if result == 0 {
        return Ok(false);
    }
    let error = io::Error::last_os_error();
    // Only ECONNREFUSED proves a pathname socket lacks a listener. In-progress,
    // backlog saturation, access failures and disappearance all fail closed.
    if error.raw_os_error() == Some(libc::ECONNREFUSED) {
        Ok(true)
    } else {
        Err(error)
    }
}

/// Raise the open-file soft limit as far as the hard limit allows (capped, since
/// macOS reports RLIM_INFINITY but rejects soft limits above OPEN_MAX).
pub fn raise_fd_limit() {
    use nix::sys::resource::{Resource, getrlimit, setrlimit};
    const TARGET: u64 = 10_240;
    let Ok((soft, hard)) = getrlimit(Resource::RLIMIT_NOFILE) else {
        return;
    };
    let want = hard.min(TARGET);
    if soft < want
        && let Err(e) = setrlimit(Resource::RLIMIT_NOFILE, want, hard)
    {
        tracing::warn!("failed to raise fd limit from {soft} to {want}: {e}");
    }
}

pub fn is_process_alive(pid: u32) -> bool {
    let Ok(raw) = i32::try_from(pid) else {
        return false;
    };
    signal::kill(Pid::from_raw(raw), None).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
    use tempfile::TempDir;

    fn private_dir() -> TempDir {
        tempfile::Builder::new()
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap()
    }

    fn write_private(path: &Path, content: &str) {
        fs::write(path, content).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }

    #[test]
    fn given_claim_pid_file_should_contain_current_pid_and_be_private() {
        let tmp = private_dir();
        let socket = tmp.path().join("daemon.sock");
        let mut guard = try_acquire_daemon_lock(&socket).unwrap().unwrap();
        guard.claim_pid_file().unwrap();
        let pid = pid_path_for(&socket);
        assert_eq!(read_pid_file(&pid).unwrap(), Some(std::process::id()));
        assert_eq!(fs::metadata(&pid).unwrap().mode() & 0o777, 0o600);
        assert_eq!(
            fs::metadata(lock_path_for(&socket)).unwrap().mode() & 0o777,
            0o600
        );
        drop(guard);
        assert!(!pid.exists());
        assert!(lock_path_for(&socket).exists());
    }

    #[test]
    fn given_stale_private_pid_should_replace_contents_while_locked() {
        let tmp = private_dir();
        let socket = tmp.path().join("daemon.sock");
        let pid = pid_path_for(&socket);
        write_private(&pid, "999999999999\n");
        let mut guard = try_acquire_daemon_lock(&socket).unwrap().unwrap();
        guard.claim_pid_file().unwrap();
        assert_eq!(read_pid_file(&pid).unwrap(), Some(std::process::id()));
    }

    #[test]
    fn given_live_socket_without_lock_should_preserve_its_pid_file() {
        let tmp = private_dir();
        let socket = tmp.path().join("daemon.sock");
        let _listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let pid = pid_path_for(&socket);
        write_private(&pid, "12345\n");
        let mut guard = try_acquire_daemon_lock(&socket).unwrap().unwrap();

        assert!(guard.claim_pid_file().is_err());
        drop(guard);
        assert_eq!(fs::read_to_string(pid).unwrap(), "12345\n");
        assert!(socket.exists());
    }

    #[test]
    fn given_missing_pid_should_return_none_without_creating_state() {
        let tmp = private_dir();
        assert_eq!(
            read_pid_file(&tmp.path().join("missing/file.pid")).unwrap(),
            None
        );
        assert!(!tmp.path().join("missing").exists());
    }

    #[test]
    fn given_unsafe_lock_or_pid_should_refuse_without_mutating_entries() {
        for suffix in ["lock", "pid"] {
            for kind in [
                "symlink",
                "dangling",
                "directory",
                "fifo",
                "shared",
                "hardlink",
            ] {
                let tmp = private_dir();
                let socket = tmp.path().join("daemon.sock");
                let target = tmp.path().join("unrelated");
                write_private(&target, "12345\n");
                let path = sibling_path(&socket, suffix);
                match kind {
                    "symlink" => symlink(&target, &path).unwrap(),
                    "dangling" => symlink(tmp.path().join("absent"), &path).unwrap(),
                    "directory" => fs::create_dir(&path).unwrap(),
                    "fifo" => nix::unistd::mkfifo(&path, nix::sys::stat::Mode::S_IRWXU).unwrap(),
                    "shared" => {
                        write_private(&path, "12345\n");
                        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
                    }
                    _ => fs::hard_link(&target, &path).unwrap(),
                }
                let before = fs::symlink_metadata(&path).unwrap();
                if suffix == "lock" {
                    assert!(try_acquire_daemon_lock(&socket).is_err(), "{suffix} {kind}");
                } else {
                    let mut guard = try_acquire_daemon_lock(&socket).unwrap().unwrap();
                    assert!(guard.claim_pid_file().is_err(), "{suffix} {kind}");
                    assert!(read_pid_file(&path).is_err(), "{suffix} {kind}");
                }
                let after = fs::symlink_metadata(&path).unwrap();
                assert_eq!(
                    (after.dev(), after.ino(), after.mode()),
                    (before.dev(), before.ino(), before.mode()),
                    "{suffix} {kind}"
                );
                assert_eq!(fs::read_to_string(&target).unwrap(), "12345\n");
            }
        }
    }

    #[test]
    fn given_old_predictable_pid_temp_symlink_should_leave_it_and_target_untouched() {
        let tmp = private_dir();
        let socket = tmp.path().join("daemon.sock");
        let target = tmp.path().join("unrelated");
        write_private(&target, "untouched");
        let old_temp = pid_path_for(&socket).with_extension("pid.tmp");
        symlink(&target, &old_temp).unwrap();
        let mut guard = try_acquire_daemon_lock(&socket).unwrap().unwrap();
        guard.claim_pid_file().unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "untouched");
        assert!(
            fs::symlink_metadata(old_temp)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    #[test]
    fn given_replaced_pid_with_same_pid_text_should_preserve_replacement_on_drop() {
        let tmp = private_dir();
        let socket = tmp.path().join("daemon.sock");
        let pid = pid_path_for(&socket);
        let mut guard = try_acquire_daemon_lock(&socket).unwrap().unwrap();
        guard.claim_pid_file().unwrap();
        fs::remove_file(&pid).unwrap();
        write_private(&pid, &format!("{}\n", std::process::id()));
        let replacement = fs::metadata(&pid).unwrap().ino();
        drop(guard);
        assert_eq!(fs::metadata(&pid).unwrap().ino(), replacement);
    }

    #[test]
    fn given_replaced_lock_should_refuse_pid_write_and_preserve_files_on_drop() {
        let tmp = private_dir();
        let socket = tmp.path().join("daemon.sock");
        let pid = pid_path_for(&socket);
        let lock = lock_path_for(&socket);
        let mut guard = try_acquire_daemon_lock(&socket).unwrap().unwrap();
        guard.claim_pid_file().unwrap();
        fs::remove_file(&lock).unwrap();
        write_private(&lock, "replacement");
        assert!(guard.claim_pid_file().is_err());
        drop(guard);
        assert!(pid.exists());
        assert_eq!(fs::read_to_string(lock).unwrap(), "replacement");
    }

    #[test]
    fn given_replaced_directory_should_not_write_or_clean_in_replacement() {
        let tmp = private_dir();
        let directory = tmp.path().join("state");
        let socket = directory.join("daemon.sock");
        let mut guard = try_acquire_daemon_lock(&socket).unwrap().unwrap();
        fs::rename(&directory, tmp.path().join("original")).unwrap();
        fs::create_dir(&directory).unwrap();
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
        write_private(&pid_path_for(&socket), "replacement");
        assert!(guard.claim_pid_file().is_err());
        drop(guard);
        assert_eq!(
            fs::read_to_string(pid_path_for(&socket)).unwrap(),
            "replacement"
        );
    }

    #[test]
    fn given_missing_state_directory_should_create_private_owned_directory() {
        let tmp = private_dir();
        let socket = tmp.path().join("new/nested/daemon.sock");
        let guard = try_acquire_daemon_lock(&socket).unwrap().unwrap();
        assert_eq!(
            fs::metadata(socket.parent().unwrap()).unwrap().mode() & 0o777,
            0o700
        );
        drop(guard);
    }

    #[test]
    fn given_unsafe_parent_or_ancestor_should_refuse_and_preserve_it() {
        for kind in ["file", "symlink", "shared_ancestor"] {
            let tmp = private_dir();
            let parent = tmp.path().join("state");
            let socket = match kind {
                "file" => {
                    fs::write(&parent, "untouched").unwrap();
                    parent.join("daemon.sock")
                }
                "symlink" => {
                    symlink(tmp.path(), &parent).unwrap();
                    parent.join("daemon.sock")
                }
                _ => {
                    fs::create_dir(&parent).unwrap();
                    fs::set_permissions(&parent, fs::Permissions::from_mode(0o777)).unwrap();
                    parent.join("nested/daemon.sock")
                }
            };
            let before = fs::symlink_metadata(&parent).unwrap();
            assert!(try_acquire_daemon_lock(&socket).is_err(), "{kind}");
            let after = fs::symlink_metadata(&parent).unwrap();
            assert_eq!((after.ino(), after.mode()), (before.ino(), before.mode()));
            assert!(!parent.join("nested").exists());
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn given_acl_grant_on_private_directory_or_ancestor_should_refuse_without_changing_it() {
        for nested in [false, true] {
            let tmp = private_dir();
            assert!(
                std::process::Command::new("/bin/chmod")
                    .args(["+a", "everyone allow list,search,add_file,delete_child"])
                    .arg(tmp.path())
                    .status()
                    .unwrap()
                    .success()
            );
            let socket = if nested {
                tmp.path().join("child/daemon.sock")
            } else {
                tmp.path().join("daemon.sock")
            };

            assert!(try_acquire_daemon_lock(&socket).is_err());
            assert_eq!(fs::metadata(tmp.path()).unwrap().mode() & 0o777, 0o700);
            assert!(!lock_path_for(&socket).exists());
        }
    }

    #[test]
    fn given_lock_held_should_refuse_second_daemon_and_release_for_next() {
        let tmp = private_dir();
        let socket = tmp.path().join("daemon.sock");
        let first = try_acquire_daemon_lock(&socket).unwrap().unwrap();
        let inode = fs::metadata(lock_path_for(&socket)).unwrap().ino();
        assert!(try_acquire_daemon_lock(&socket).unwrap().is_none());
        drop(first);
        assert!(try_acquire_daemon_lock(&socket).unwrap().is_some());
        assert_eq!(fs::metadata(lock_path_for(&socket)).unwrap().ino(), inode);
    }

    #[test]
    fn given_concurrent_first_start_should_choose_one_lock_holder_without_creation_errors() {
        use std::sync::{Arc, Barrier};
        for _ in 0..10 {
            let tmp = private_dir();
            let socket = tmp.path().join("daemon.sock");
            let barrier = Arc::new(Barrier::new(8));
            let handles: Vec<_> = (0..8)
                .map(|_| {
                    let socket = socket.clone();
                    let barrier = barrier.clone();
                    std::thread::spawn(move || {
                        barrier.wait();
                        let result = try_acquire_daemon_lock(&socket);
                        barrier.wait();
                        result.map(|guard| guard.is_some())
                    })
                })
                .collect();
            let results: Vec<_> = handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .collect();
            assert!(results.iter().all(Result::is_ok), "{results:?}");
            assert_eq!(
                results
                    .into_iter()
                    .filter(|r| matches!(r, Ok(true)))
                    .count(),
                1
            );
        }
    }

    #[test]
    fn given_pid_should_check_if_process_alive() {
        assert!(is_process_alive(std::process::id()));
        assert!(!is_process_alive(99999999));
    }

    #[test]
    fn given_distinct_socket_names_should_have_distinct_metadata_paths() {
        let a = Path::new("/x/daemon.sock");
        let b = Path::new("/x/daemon");
        assert_eq!(lock_path_for(a), Path::new("/x/daemon.sock.lock"));
        assert_eq!(pid_path_for(a), Path::new("/x/daemon.sock.pid"));
        assert_ne!(lock_path_for(a), lock_path_for(b));
        assert_ne!(pid_path_for(a), pid_path_for(b));
    }
}

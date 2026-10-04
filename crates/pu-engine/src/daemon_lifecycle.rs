use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use nix::errno::Errno;
use nix::fcntl::{Flock, FlockArg};
use nix::sys::signal;
use nix::unistd::Pid;

/// The exclusive lock that makes a pu-engine the one daemon for its socket.
///
/// Held for the life of the process. The kernel drops it however the process
/// dies, so a crashed daemon never leaves a stale lock behind.
pub struct DaemonLock {
    _lock: Flock<File>,
}

/// Lock file guarding `socket_path` (`daemon.sock` -> `daemon.lock`).
pub fn lock_path_for(socket_path: &Path) -> PathBuf {
    socket_path.with_extension("lock")
}

/// PID file for the daemon on `socket_path` (`daemon.sock` -> `daemon.pid`).
pub fn pid_path_for(socket_path: &Path) -> PathBuf {
    socket_path.with_extension("pid")
}

/// Take the daemon lock for `socket_path` without blocking.
///
/// Returns `Ok(None)` when another live daemon already holds it. Every daemon
/// must hold this before touching the socket: binding unlinks the existing
/// socket file, so a second daemon would otherwise steal the path and orphan
/// every agent the first one is running.
pub fn try_acquire_daemon_lock(socket_path: &Path) -> Result<Option<DaemonLock>, std::io::Error> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(lock_path_for(socket_path))?;
    match Flock::lock(file, FlockArg::LockExclusiveNonblock) {
        Ok(lock) => Ok(Some(DaemonLock { _lock: lock })),
        Err((_, Errno::EWOULDBLOCK)) => Ok(None),
        Err((_, e)) => Err(e.into()),
    }
}

/// Record this process as the daemon. Only call while holding the
/// [`DaemonLock`]: any existing PID file is then stale and is replaced.
pub fn claim_pid_file(path: &Path) -> Result<(), std::io::Error> {
    let tmp = path.with_extension("pid.tmp");
    {
        let mut file = File::create(&tmp)?;
        writeln!(file, "{}", std::process::id())?;
        file.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

pub fn read_pid_file(path: &Path) -> Result<Option<u32>, std::io::Error> {
    match std::fs::read_to_string(path) {
        Ok(content) => Ok(content.trim().parse().ok()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Remove the PID file and socket on shutdown, but only if the PID file still
/// names this process: a successor daemon may already own both paths.
pub fn cleanup_files(pid_path: &Path, socket_path: &Path) {
    if !matches!(read_pid_file(pid_path), Ok(Some(pid)) if pid == std::process::id()) {
        return;
    }
    let _ = std::fs::remove_file(pid_path);
    let _ = std::fs::remove_file(socket_path);
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
    use tempfile::TempDir;

    #[test]
    fn given_claim_pid_file_should_contain_current_pid() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("daemon.pid");
        claim_pid_file(&path).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let pid: u32 = content.trim().parse().unwrap();
        assert_eq!(pid, std::process::id());
    }

    #[test]
    fn given_read_pid_file_should_return_pid() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("daemon.pid");
        std::fs::write(&path, "12345\n").unwrap();

        let pid = read_pid_file(&path).unwrap();
        assert_eq!(pid, Some(12345));
    }

    #[test]
    fn given_missing_pid_file_should_return_none() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("daemon.pid");
        let pid = read_pid_file(&path).unwrap();
        assert_eq!(pid, None);
    }

    #[test]
    fn given_cleanup_should_remove_own_pid_and_socket() {
        let tmp = TempDir::new().unwrap();
        let pid_path = tmp.path().join("daemon.pid");
        let sock_path = tmp.path().join("daemon.sock");
        claim_pid_file(&pid_path).unwrap();
        std::fs::write(&sock_path, "").unwrap();

        cleanup_files(&pid_path, &sock_path);

        assert!(!pid_path.exists());
        assert!(!sock_path.exists());
    }

    #[test]
    fn given_cleanup_when_successor_owns_pid_file_should_leave_files() {
        let tmp = TempDir::new().unwrap();
        let pid_path = tmp.path().join("daemon.pid");
        let sock_path = tmp.path().join("daemon.sock");
        std::fs::write(&pid_path, "999999\n").unwrap();
        std::fs::write(&sock_path, "").unwrap();

        cleanup_files(&pid_path, &sock_path);

        assert!(pid_path.exists());
        assert!(sock_path.exists());
    }

    #[test]
    fn given_cleanup_with_missing_files_should_not_error() {
        let tmp = TempDir::new().unwrap();
        let pid_path = tmp.path().join("nonexistent.pid");
        let sock_path = tmp.path().join("nonexistent.sock");

        // Should not panic
        cleanup_files(&pid_path, &sock_path);
    }

    #[test]
    fn given_pid_should_check_if_process_alive() {
        // Current process is always alive
        let alive = is_process_alive(std::process::id());
        assert!(alive);
    }

    #[test]
    fn given_bogus_pid_should_report_not_alive() {
        // PID 99999999 is almost certainly not running
        let alive = is_process_alive(99999999);
        assert!(!alive);
    }

    #[test]
    fn given_claim_pid_file_should_replace_stale_pid() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("daemon.pid");
        std::fs::write(&path, "12345\n").unwrap();

        claim_pid_file(&path).unwrap();

        assert_eq!(read_pid_file(&path).unwrap(), Some(std::process::id()));
    }

    #[test]
    fn given_lock_held_should_refuse_second_daemon() {
        let tmp = TempDir::new().unwrap();
        let sock = tmp.path().join("daemon.sock");

        let first = try_acquire_daemon_lock(&sock).unwrap();
        assert!(first.is_some());
        assert!(try_acquire_daemon_lock(&sock).unwrap().is_none());
    }

    #[test]
    fn given_lock_released_should_allow_next_daemon() {
        let tmp = TempDir::new().unwrap();
        let sock = tmp.path().join("daemon.sock");

        drop(try_acquire_daemon_lock(&sock).unwrap());

        assert!(try_acquire_daemon_lock(&sock).unwrap().is_some());
    }

    #[test]
    fn given_socket_path_should_derive_sibling_lock_and_pid_paths() {
        let sock = Path::new("/x/.pu/daemon.sock");
        assert_eq!(lock_path_for(sock), Path::new("/x/.pu/daemon.lock"));
        assert_eq!(pid_path_for(sock), Path::new("/x/.pu/daemon.pid"));
    }
}

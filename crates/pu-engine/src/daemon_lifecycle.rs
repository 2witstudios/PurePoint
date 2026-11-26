use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

#[cfg(unix)]
use nix::sys::signal;
#[cfg(unix)]
use nix::unistd::Pid;

/// The exclusive lock that makes a pu-engine the one daemon for its socket.
///
/// Held for the life of the process. The kernel drops it however the process
/// dies, so a crashed daemon never leaves a stale lock behind.
pub struct DaemonLock {
    /// Locked with `flock` (Unix) or `LockFileEx` (Windows) through fs4;
    /// closing the handle releases it.
    _lock: File,
}

/// Lock file guarding `socket_path` (`daemon.sock` -> `daemon.sock.lock`).
pub fn lock_path_for(socket_path: &Path) -> PathBuf {
    sibling_path(socket_path, "lock")
}

/// PID file for the daemon on `socket_path` (`daemon.sock` -> `daemon.sock.pid`).
pub fn pid_path_for(socket_path: &Path) -> PathBuf {
    sibling_path(socket_path, "pid")
}

/// `<socket>.<ext>`: appending to the full path keeps the mapping one-to-one,
/// so distinct sockets (`pu.sock`, `pu.test`, `daemon`) never share a lock or
/// PID file.
fn sibling_path(socket_path: &Path, ext: &str) -> PathBuf {
    let mut name = socket_path.as_os_str().to_owned();
    name.push(".");
    name.push(ext);
    PathBuf::from(name)
}

/// Create the directory that holds `socket_path`, missing parents at mode 0700.
///
/// When that directory is the global `~/.pu`, it also holds the registry,
/// config and logs, so it is forced to 0700 (repairing a group- or
/// other-accessible one), and a `~/.pu` owned by another user is refused.
/// Other existing directories are left as they are; the socket itself is
/// owner-only either way.
///
/// Windows: "mode 0700" is the owner-only protected DACL of
/// `pu_core::private_fs`, and "owned by another user" is an owner SID that
/// is neither this user nor SYSTEM/Administrators.
#[cfg(windows)]
pub fn prepare_socket_dir(socket_path: &Path, global_dir: Option<&Path>) -> std::io::Result<()> {
    use pu_core::private_fs;
    let Some(dir) = socket_path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
    else {
        return Ok(());
    };
    if global_dir != Some(dir) {
        if !dir.exists() {
            private_fs::create_private_dir(dir)?;
        }
        return Ok(());
    }
    // Rewriting the DACL needs WRITE_DAC, which only the owner holds by
    // default; the check then refuses a directory owned by someone else.
    private_fs::create_private_dir(dir)?;
    private_fs::check_private(dir)
}

/// Create the directory that holds `socket_path`, missing parents at mode 0700.
///
/// When that directory is the global `~/.pu`, it also holds the registry,
/// config and logs, so it is forced to 0700 (repairing a group- or
/// other-accessible one), and a `~/.pu` owned by another user is refused.
/// Other existing directories are left as they are; the socket itself is
/// owner-only either way.
#[cfg(unix)]
pub fn prepare_socket_dir(socket_path: &Path, global_dir: Option<&Path>) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt};
    let Some(dir) = socket_path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
    else {
        return Ok(());
    };
    if global_dir != Some(dir) {
        if !dir.exists() {
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(dir)?;
        }
        return Ok(());
    }
    match std::fs::metadata(dir) {
        // SAFETY: geteuid has no preconditions and cannot fail.
        Ok(meta) if meta.uid() != unsafe { libc::geteuid() } => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!("{} is owned by another user", dir.display()),
            ));
        }
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    pu_core::machines::ensure_private_dir(dir).map_err(std::io::Error::other)
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
    match fs4::FileExt::try_lock(&file) {
        Ok(()) => Ok(Some(DaemonLock { _lock: file })),
        Err(fs4::TryLockError::WouldBlock) => Ok(None),
        Err(fs4::TryLockError::Error(e)) => Err(e),
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
///
/// Windows has no descriptor limit for handles (the CRT's `_setmaxstdio`
/// only bounds C `FILE*` streams, which the daemon does not use): a no-op.
#[cfg(windows)]
pub fn raise_fd_limit() {}

/// Raise the open-file soft limit as far as the hard limit allows (capped, since
/// macOS reports RLIM_INFINITY but rejects soft limits above OPEN_MAX).
#[cfg(unix)]
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

/// A process we may query that has not exited. Like `kill(pid, 0)` failing
/// with EPERM on Unix, a process this user cannot open counts as not alive.
#[cfg(windows)]
pub fn is_process_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    // SAFETY: OpenProcess has no preconditions; a null handle means failure.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return false;
    }
    let mut code = 0u32;
    // SAFETY: handle is a live process handle we own and close once.
    let queried = unsafe { GetExitCodeProcess(handle, &mut code) } != 0;
    unsafe { CloseHandle(handle) };
    queried && code == STILL_ACTIVE as u32
}

/// Point stderr at `~/.pu/daemon.log` when the launcher passed no stderr
/// handle (a Run-value autostart). The daemon is GUI-subsystem, so it has no
/// console to fall back on; std and tracing look the handle up on every
/// write, so both follow the replacement.
#[cfg(windows)]
pub fn ensure_stderr_log() {
    use std::os::windows::io::IntoRawHandle;
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Console::{GetStdHandle, STD_ERROR_HANDLE, SetStdHandle};
    // SAFETY: GetStdHandle has no preconditions.
    let current = unsafe { GetStdHandle(STD_ERROR_HANDLE) };
    if !current.is_null() && current != INVALID_HANDLE_VALUE {
        return;
    }
    let Ok(log_path) = pu_core::paths::daemon_log_path() else {
        return;
    };
    if let Some(parent) = log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let Ok(file) = OpenOptions::new().create(true).append(true).open(&log_path) else {
        return;
    };
    // Leaked on purpose: stderr stays open for the life of the process.
    let handle = file.into_raw_handle();
    // SAFETY: handle is a valid file handle that is never closed.
    unsafe { SetStdHandle(STD_ERROR_HANDLE, handle) };
}

/// Block until process `pid` exits (returns at once if it already has or
/// cannot be opened): `--parent-pid` waits on the parent's handle.
#[cfg(windows)]
pub fn wait_for_process_exit(pid: u32) {
    use windows_sys::Win32::Foundation::{CloseHandle, WAIT_FAILED};
    use windows_sys::Win32::System::Threading::{
        INFINITE, OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
    };
    // SAFETY: OpenProcess has no preconditions; a null handle means failure.
    let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
    if handle.is_null() {
        return;
    }
    // SAFETY: handle is a live process handle we own and close once.
    let waited = unsafe { WaitForSingleObject(handle, INFINITE) };
    unsafe { CloseHandle(handle) };
    if waited == WAIT_FAILED {
        tracing::warn!(pid, "waiting on the parent process failed");
    }
}

#[cfg(unix)]
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

    #[cfg(unix)]
    fn mode(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[cfg(unix)]
    #[test]
    fn given_missing_global_dir_should_create_it_owner_only() {
        let tmp = TempDir::new().unwrap();
        let global = tmp.path().join(".pu");
        prepare_socket_dir(&global.join("daemon.sock"), Some(&global)).unwrap();
        assert_eq!(mode(&global), 0o700);
    }

    #[cfg(unix)]
    #[test]
    fn given_shared_global_dir_should_repair_it_to_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = TempDir::new().unwrap();
        let global = tmp.path().join(".pu");
        for shared in [0o755, 0o775, 0o777] {
            std::fs::create_dir_all(&global).unwrap();
            std::fs::set_permissions(&global, std::fs::Permissions::from_mode(shared)).unwrap();
            prepare_socket_dir(&global.join("daemon.sock"), Some(&global)).unwrap();
            assert_eq!(mode(&global), 0o700, "from {shared:o}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn given_global_dir_owned_by_another_user_should_refuse() {
        // Any directory we do not own will do: `/` is root's (skip as root).
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let root = Path::new("/");
        let err = prepare_socket_dir(&root.join("daemon.sock"), Some(root)).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
    }

    #[cfg(unix)]
    #[test]
    fn given_custom_socket_dir_should_create_missing_but_keep_existing_modes() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = TempDir::new().unwrap();
        let global = tmp.path().join(".pu");
        let fresh = tmp.path().join("fresh");
        prepare_socket_dir(&fresh.join("pu.sock"), Some(&global)).unwrap();
        assert_eq!(mode(&fresh), 0o700);

        let existing = tmp.path().join("existing");
        std::fs::create_dir(&existing).unwrap();
        std::fs::set_permissions(&existing, std::fs::Permissions::from_mode(0o755)).unwrap();
        prepare_socket_dir(&existing.join("pu.sock"), Some(&global)).unwrap();
        assert_eq!(mode(&existing), 0o755);
        assert!(!global.exists());
    }

    #[cfg(windows)]
    #[test]
    fn given_missing_or_shared_global_dir_should_make_it_private() {
        let tmp = TempDir::new().unwrap();
        let global = tmp.path().join(".pu");
        prepare_socket_dir(&global.join("daemon.sock"), Some(&global)).unwrap();
        pu_core::private_fs::check_private(&global).unwrap();
        pu_core::win_security::grant_for_test(&global, "WD").unwrap(); // Everyone
        assert!(pu_core::private_fs::check_private(&global).is_err());
        prepare_socket_dir(&global.join("daemon.sock"), Some(&global)).unwrap();
        pu_core::private_fs::check_private(&global).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn given_custom_socket_dir_should_create_missing_private_but_keep_existing() {
        let tmp = TempDir::new().unwrap();
        let global = tmp.path().join(".pu");
        let fresh = tmp.path().join("fresh");
        prepare_socket_dir(&fresh.join("pu.sock"), Some(&global)).unwrap();
        pu_core::private_fs::check_private(&fresh).unwrap();

        let existing = tmp.path().join("existing");
        std::fs::create_dir(&existing).unwrap();
        pu_core::win_security::grant_for_test(&existing, "WD").unwrap();
        prepare_socket_dir(&existing.join("pu.sock"), Some(&global)).unwrap();
        assert!(pu_core::private_fs::check_private(&existing).is_err());
        assert!(!global.exists());
    }

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
        assert_eq!(lock_path_for(sock), Path::new("/x/.pu/daemon.sock.lock"));
        assert_eq!(pid_path_for(sock), Path::new("/x/.pu/daemon.sock.pid"));
    }

    #[test]
    fn given_distinct_custom_sockets_should_not_share_lock_or_pid_paths() {
        let a = Path::new("/tmp/pu.sock");
        let b = Path::new("/tmp/pu.test");
        let c = Path::new("/tmp/pu");
        let canonical = Path::new("/tmp/daemon.sock");
        let bare = Path::new("/tmp/daemon");
        assert_ne!(lock_path_for(canonical), lock_path_for(bare));
        assert_ne!(pid_path_for(canonical), pid_path_for(bare));
        assert_eq!(lock_path_for(a), Path::new("/tmp/pu.sock.lock"));
        assert_eq!(pid_path_for(b), Path::new("/tmp/pu.test.pid"));
        assert_ne!(lock_path_for(a), lock_path_for(b));
        assert_ne!(lock_path_for(a), lock_path_for(c));
        assert_ne!(pid_path_for(b), pid_path_for(c));
    }
}

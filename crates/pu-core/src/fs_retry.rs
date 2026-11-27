//! `rename`, `remove_file` and `remove_dir_all` that wait out Windows file
//! locks (WIN-3.13, REQ-WINAPP-045/046).
//!
//! On Windows an open handle (a file watcher, an editor, Defender or the
//! search indexer, an agent that is still exiting) makes delete and rename
//! fail with `ERROR_SHARING_VIOLATION`, `ERROR_ACCESS_DENIED` or
//! `ERROR_LOCK_VIOLATION`, which Unix never raises. These wrappers clear
//! read-only attributes (git pack files are read-only) and retry with a
//! bounded backoff: at least [`MIN_TRIES`] attempts over at least
//! [`BUDGET`]. When the budget runs out the error names the path that is
//! still locked.
//!
//! On Unix every function is a plain passthrough to `std::fs`.
use std::io;
use std::path::Path;
#[cfg(windows)]
use std::path::PathBuf;
use std::time::Duration;

/// Minimum number of attempts before giving up.
pub const MIN_TRIES: u32 = 10;
/// Minimum time spent retrying before giving up.
pub const BUDGET: Duration = Duration::from_secs(2);

/// `std::fs::rename`, retried while `from` or `to` is locked.
pub fn rename(from: impl AsRef<Path>, to: impl AsRef<Path>) -> io::Result<()> {
    let (from, to) = (from.as_ref(), to.as_ref());
    #[cfg(unix)]
    return std::fs::rename(from, to);
    #[cfg(windows)]
    return retry(
        || std::fs::rename(from, to),
        |_| clear_readonly(to),
        || from.into(),
    );
}

/// `std::fs::remove_file`, retried while `path` is locked.
pub fn remove_file(path: impl AsRef<Path>) -> io::Result<()> {
    let path = path.as_ref();
    #[cfg(unix)]
    return std::fs::remove_file(path);
    #[cfg(windows)]
    return retry(
        || std::fs::remove_file(path),
        |_| clear_readonly(path),
        || path.into(),
    );
}

/// `std::fs::remove_dir`, retried while `path` is locked.
pub fn remove_dir(path: impl AsRef<Path>) -> io::Result<()> {
    let path = path.as_ref();
    #[cfg(unix)]
    return std::fs::remove_dir(path);
    #[cfg(windows)]
    return retry(
        || std::fs::remove_dir(path),
        |_| clear_readonly(path),
        || path.into(),
    );
}

/// `std::fs::remove_dir_all`, retried while anything inside is locked. The
/// final error names the first entry that is still held open.
pub fn remove_dir_all(path: impl AsRef<Path>) -> io::Result<()> {
    let path = path.as_ref();
    #[cfg(unix)]
    return std::fs::remove_dir_all(path);
    #[cfg(windows)]
    return retry(
        || std::fs::remove_dir_all(path),
        |first| {
            if first {
                clear_readonly_tree(path);
            }
        },
        || locked_entry(path).unwrap_or_else(|| path.into()),
    );
}

/// Run `op` with the same retry policy (for callers with their own
/// replace/delete primitive, such as `ReplaceFileW`).
pub fn retry_op<T>(path: &Path, op: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    #[cfg(unix)]
    {
        let _ = path;
        let mut op = op;
        op()
    }
    #[cfg(windows)]
    retry(op, |_| clear_readonly(path), || path.into())
}

/// True for the Windows errors an open handle causes.
pub fn is_lock_error(e: &io::Error) -> bool {
    const ERROR_ACCESS_DENIED: i32 = 5;
    const ERROR_SHARING_VIOLATION: i32 = 32;
    const ERROR_LOCK_VIOLATION: i32 = 33;
    cfg!(windows)
        && matches!(
            e.raw_os_error(),
            Some(ERROR_ACCESS_DENIED | ERROR_SHARING_VIOLATION | ERROR_LOCK_VIOLATION)
        )
}

/// Delay before attempt `n` (1-based retries): 10 ms doubling to 250 ms.
#[cfg(windows)]
fn backoff(n: u32) -> Duration {
    Duration::from_millis((10u64 << n.min(5)).min(250))
}

#[cfg(windows)]
fn retry<T>(
    mut op: impl FnMut() -> io::Result<T>,
    mut prepare: impl FnMut(bool),
    locked: impl FnOnce() -> PathBuf,
) -> io::Result<T> {
    let start = std::time::Instant::now();
    let mut tries = 0u32;
    loop {
        tries += 1;
        let e = match op() {
            Ok(v) => return Ok(v),
            Err(e) if is_lock_error(&e) => e,
            Err(e) => return Err(e),
        };
        if tries >= MIN_TRIES && start.elapsed() >= BUDGET {
            return Err(name_locked(&locked(), exhausted(e, tries, start.elapsed())));
        }
        prepare(tries == 1);
        std::thread::sleep(backoff(tries));
    }
}

#[cfg(windows)]
fn exhausted(e: io::Error, tries: u32, elapsed: Duration) -> io::Error {
    io::Error::new(
        e.kind(),
        format!(
            "{e}; still locked after {tries} tries over {:.1}s",
            elapsed.as_secs_f32()
        ),
    )
}

#[cfg(windows)]
fn name_locked(path: &Path, e: io::Error) -> io::Error {
    io::Error::new(
        e.kind(),
        format!(
            "{} is locked by another process (an editor, a file watcher, antivirus or an \
             exiting agent may hold it open): {e}",
            path.display()
        ),
    )
}

#[cfg(windows)]
fn clear_readonly(path: &Path) {
    if let Ok(meta) = std::fs::symlink_metadata(path) {
        let mut perms = meta.permissions();
        if perms.readonly() && !meta.file_type().is_symlink() {
            #[allow(clippy::permissions_set_readonly_false)]
            perms.set_readonly(false);
            let _ = std::fs::set_permissions(path, perms);
        }
    }
}

#[cfg(windows)]
fn clear_readonly_tree(path: &Path) {
    clear_readonly(path);
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return;
    };
    // Never follow a link or junction out of the tree.
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return;
    }
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            clear_readonly_tree(&entry.path());
        }
    }
}

/// The first entry under `path` some other process holds open: an exclusive
/// open (share mode 0) fails on exactly those.
#[cfg(windows)]
fn locked_entry(path: &Path) -> Option<PathBuf> {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    let meta = std::fs::symlink_metadata(path).ok()?;
    if meta.is_dir() && !meta.file_type().is_symlink() {
        for entry in std::fs::read_dir(path).ok()?.flatten() {
            if let Some(found) = locked_entry(&entry.path()) {
                return Some(found);
            }
        }
    }
    let opened = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path);
    match opened {
        Err(e) if is_lock_error(&e) => Some(path.to_path_buf()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_unlocked_paths_should_pass_through() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("a");
        std::fs::write(&a, b"x").unwrap();
        let b = tmp.path().join("b");
        rename(&a, &b).unwrap();
        remove_file(&b).unwrap();
        let dir = tmp.path().join("d");
        std::fs::create_dir_all(dir.join("e")).unwrap();
        std::fs::write(dir.join("e").join("f"), b"x").unwrap();
        remove_dir_all(&dir).unwrap();
        assert!(!dir.exists());
        std::fs::create_dir(&dir).unwrap();
        remove_dir(&dir).unwrap();
        // Missing paths fail at once, never after the retry budget.
        let start = std::time::Instant::now();
        assert!(remove_file(&b).is_err());
        assert!(remove_dir_all(&dir).is_err());
        assert!(start.elapsed() < Duration::from_millis(500));
    }

    #[cfg(windows)]
    #[test]
    fn given_read_only_tree_should_remove_it() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("pack");
        std::fs::create_dir(&dir).unwrap();
        let file = dir.join("pack-1.idx");
        std::fs::write(&file, b"x").unwrap();
        let mut perms = std::fs::metadata(&file).unwrap().permissions();
        perms.set_readonly(true);
        std::fs::set_permissions(&file, perms).unwrap();
        remove_dir_all(&dir).unwrap();
        assert!(!dir.exists());
    }

    #[cfg(windows)]
    #[test]
    fn given_backoff_should_cover_the_budget_in_enough_tries() {
        let total: Duration = (1..MIN_TRIES).map(backoff).sum();
        assert!(total < BUDGET, "ten tries fit inside the budget: {total:?}");
        assert_eq!(backoff(1), Duration::from_millis(20));
        assert_eq!(backoff(9), Duration::from_millis(250));
    }

    /// Another process holds a file open (no sharing): removal retries for
    /// the whole budget, then names the file; once it closes, removal works.
    #[cfg(windows)]
    #[test]
    fn given_file_held_open_by_another_process_should_name_it_then_succeed_after_close() {
        use std::io::{BufRead, Write};
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("worktree");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        let held = dir.join("src").join("held.rs");
        std::fs::write(&held, b"x").unwrap();
        let mut holder = hold_open(&held);
        let start = std::time::Instant::now();
        let err = remove_dir_all(&dir).unwrap_err();
        assert!(start.elapsed() >= BUDGET, "{:?}", start.elapsed());
        let message = err.to_string();
        assert!(message.contains(&held.display().to_string()), "{message}");
        assert!(message.contains("locked"), "{message}");
        // rename of the held file is refused the same way.
        let err = rename(&held, dir.join("moved.rs")).unwrap_err();
        assert!(err.to_string().contains("locked"), "{err}");

        writeln!(holder.stdin.take().unwrap()).unwrap();
        let mut line = String::new();
        std::io::BufReader::new(holder.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        assert!(holder.wait().unwrap().success());
        remove_dir_all(&dir).unwrap();
        assert!(!dir.exists());
    }

    /// Release while a retry is waiting: the removal succeeds within budget.
    #[cfg(windows)]
    #[test]
    fn given_lock_released_during_retry_should_succeed() {
        use std::io::Write;
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("busy.txt");
        std::fs::write(&file, b"x").unwrap();
        let mut holder = hold_open(&file);
        let mut stdin = holder.stdin.take().unwrap();
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(400));
            writeln!(stdin).unwrap();
        });
        remove_file(&file).unwrap();
        assert!(!file.exists());
        release.join().unwrap();
        holder.wait().unwrap();
    }

    /// A PowerShell process that opens `path` with no sharing, prints
    /// `ready`, and closes it when a line arrives on stdin.
    #[cfg(windows)]
    pub(crate) fn hold_open(path: &Path) -> std::process::Child {
        use std::io::BufRead;
        let script = format!(
            "$f = [System.IO.File]::Open('{}', 'Open', 'Read', 'None'); \
             [Console]::Out.WriteLine('ready'); [Console]::Out.Flush(); \
             [void][Console]::In.ReadLine(); $f.Close(); \
             [Console]::Out.WriteLine('closed')",
            path.display().to_string().replace('\'', "''")
        );
        let mut child = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdout = std::io::BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        stdout.read_line(&mut line).unwrap();
        assert_eq!(line.trim(), "ready");
        child.stdout = Some(stdout.into_inner());
        child
    }
}

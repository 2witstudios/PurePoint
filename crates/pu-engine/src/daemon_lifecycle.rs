use std::io::Write;
use std::path::Path;

use nix::sys::signal;
use nix::unistd::Pid;

pub fn write_pid_file(path: &Path) -> Result<(), std::io::Error> {
    let open_exclusive = || {
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
    };
    let mut file = match open_exclusive() {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            // Only remove the PID file if we can parse a PID and that process is dead.
            // If the file is empty, corrupted, or unreadable, treat it as in-use to
            // avoid racing with a daemon that created the file but hasn't written yet.
            match read_pid_file(path) {
                Ok(Some(pid)) if !is_process_alive(pid) => {
                    let _ = std::fs::remove_file(path);
                    open_exclusive()?
                }
                Ok(Some(pid)) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::AlreadyExists,
                        format!("daemon already running (pid {})", pid),
                    ));
                }
                _ => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::AlreadyExists,
                        "daemon already running (PID file exists)",
                    ));
                }
            }
        }
        Err(e) => return Err(e),
    };
    writeln!(file, "{}", std::process::id())?;
    Ok(())
}

pub fn read_pid_file(path: &Path) -> Result<Option<u32>, std::io::Error> {
    match std::fs::read_to_string(path) {
        Ok(content) => Ok(content.trim().parse().ok()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

pub fn cleanup_files(pid_path: &Path, socket_path: &Path) {
    let _ = std::fs::remove_file(pid_path);
    let _ = std::fs::remove_file(socket_path);
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
    fn given_write_pid_file_should_contain_current_pid() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("daemon.pid");
        write_pid_file(&path).unwrap();

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
    fn given_cleanup_should_remove_pid_and_socket() {
        let tmp = TempDir::new().unwrap();
        let pid_path = tmp.path().join("daemon.pid");
        let sock_path = tmp.path().join("daemon.sock");
        std::fs::write(&pid_path, "999").unwrap();
        std::fs::write(&sock_path, "").unwrap();

        cleanup_files(&pid_path, &sock_path);

        assert!(!pid_path.exists());
        assert!(!sock_path.exists());
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
    fn given_existing_pid_file_should_fail_with_already_exists() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("daemon.pid");
        write_pid_file(&path).unwrap();

        let result = write_pid_file(&path);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::AlreadyExists);
    }
}

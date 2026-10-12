use std::path::{Path, PathBuf};

use pu_core::paths;
use pu_core::protocol::{PROTOCOL_VERSION, Request, Response};

use crate::error::CliError;

/// Validate the complete ancestor/ACL chain before creating private state.
fn prepare_state_directory(path: &Path) -> Result<(), CliError> {
    pu_core::private_fs::PrivateDir::open(path, true)?;
    Ok(())
}

fn open_daemon_log(path: &Path) -> Result<std::fs::File, CliError> {
    let parent = path
        .parent()
        .ok_or_else(|| CliError::Other("daemon log needs a parent directory".into()))?;
    let name = path
        .file_name()
        .ok_or_else(|| CliError::Other("daemon log needs a basename".into()))?;
    let dir = pu_core::private_fs::PrivateDir::open(parent, false)?;
    Ok(dir.open_append(name)?)
}

pub fn find_daemon_binary() -> Option<PathBuf> {
    which::which("pu-engine").ok()
}

/// Longest a single health probe may take. A daemon that accepts but never
/// answers would otherwise hold each probe for the full request timeout.
const HEALTH_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

/// Check compatibility before any command; never restart a live daemon because
/// doing so would terminate its agent sessions.
pub async fn check_daemon_health(socket: &Path) -> Result<bool, CliError> {
    let probe = crate::client::send_request(socket, &Request::Health);
    match tokio::time::timeout(HEALTH_PROBE_TIMEOUT, probe).await {
        Ok(Ok(Response::HealthReport {
            protocol_version, ..
        })) => {
            if protocol_version != PROTOCOL_VERSION {
                return Err(CliError::Other(format!(
                    "daemon protocol v{protocol_version} is incompatible with CLI protocol v{PROTOCOL_VERSION} at {} — update pu and pu-engine together and restart the daemon after saving active agent work",
                    socket.display()
                )));
            }
            Ok(true)
        }
        // BUSY establishes liveness, but cannot establish protocol compatibility.
        Ok(Ok(Response::Error { code, message })) if code == "BUSY" => {
            Err(CliError::DaemonError { code, message })
        }
        _ => Ok(false),
    }
}

pub async fn ensure_daemon(socket: &Path) -> Result<(), CliError> {
    // Retry before concluding there is no daemon: a busy or restarting daemon
    // can miss one check. (A daemon started needlessly exits on its own, since
    // pu-engine holds a single-instance lock, but the retry avoids the churn.)
    for attempt in 0..3 {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
        if check_daemon_health(socket).await? {
            return Ok(());
        }
    }

    let binary = find_daemon_binary().ok_or(CliError::Other(
        "pu-engine not found on PATH — install with `cargo install --path crates/pu-engine`".into(),
    ))?;

    if let Some(parent) = socket.parent().filter(|path| !path.as_os_str().is_empty()) {
        prepare_state_directory(parent)?;
    }

    // Redirect daemon stderr to log file so startup errors are diagnosable
    let log_path = paths::daemon_log_path()?;
    if let Some(parent) = log_path.parent() {
        prepare_state_directory(parent)?;
    }
    let stderr_target = std::process::Stdio::from(open_daemon_log(&log_path)?);

    // Start daemon
    std::process::Command::new(&binary)
        .arg("--socket")
        .arg(socket)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(stderr_target)
        .spawn()
        .map_err(CliError::Io)?;

    // Poll health with exponential backoff: 10, 20, 40, 80, 160, 320, 640ms.
    // The 3s budget is wall-clock and includes the probes themselves, which can
    // each take up to HEALTH_PROBE_TIMEOUT against a daemon that never answers.
    let poll = async {
        let mut delay_ms = 10u64;
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
            if check_daemon_health(socket).await? {
                return Ok::<(), CliError>(());
            }
            delay_ms = (delay_ms * 2).min(640);
        }
    };
    tokio::time::timeout(std::time::Duration::from_secs(3), poll)
        .await
        .map_err(|_| CliError::Other(
            "daemon did not start within 3 seconds; inspect ~/.pu/daemon.log for private-state errors. Existing directories must be owned 0700 and lock/PID files owned, single-link 0600 regular files. Stop the daemon before deliberately migrating old state; unsafe state is never repaired automatically".into()
        ))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_public_ancestor_should_not_create_state_or_log() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let tmp = tempfile::TempDir::new().unwrap();
        let parent = tmp.path().join("public");
        std::fs::create_dir(&parent).unwrap();
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o777)).unwrap();
        let state = parent.join("state");
        std::fs::create_dir(&state).unwrap();
        std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o700)).unwrap();
        let log = state.join("daemon.log");
        assert!(open_daemon_log(&log).is_err());
        assert!(!log.exists());
        let missing = parent.join("missing");
        assert!(prepare_state_directory(&missing).is_err());
        assert!(!missing.exists());
        assert_eq!(std::fs::metadata(&parent).unwrap().mode() & 0o7777, 0o777);
    }

    #[test]
    fn given_unsafe_log_should_reject_without_blocking_or_changing_existing_entries() {
        use std::io::Write;
        use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
        // given
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::set_permissions(tmp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        for kind in ["public", "link", "hardlink", "fifo", "directory"] {
            let path = tmp.path().join(kind);
            let target = tmp.path().join(format!("{kind}-target"));
            std::fs::write(&target, b"preserved").unwrap();
            std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).unwrap();
            match kind {
                "public" => {
                    std::fs::write(&path, b"preserved").unwrap();
                    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
                        .unwrap();
                }
                "link" => symlink(&target, &path).unwrap(),
                "hardlink" => std::fs::hard_link(&target, &path).unwrap(),
                "directory" => std::fs::create_dir(&path).unwrap(),
                _ => {
                    use std::os::unix::ffi::OsStrExt;
                    let name = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
                    // SAFETY: valid NUL-terminated path in this scratch directory.
                    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
                }
            }
            let before = std::fs::symlink_metadata(&path).unwrap();
            // when / then: FIFO has no reader; a blocking open would hang here.
            assert!(open_daemon_log(&path).is_err(), "{kind}");
            let after = std::fs::symlink_metadata(&path).unwrap();
            assert_eq!(
                (before.ino(), before.mode(), before.len()),
                (after.ino(), after.mode(), after.len())
            );
            assert_eq!(std::fs::read(&target).unwrap(), b"preserved");
        }
        let safe = tmp.path().join("private");
        open_daemon_log(&safe).unwrap().write_all(b"one").unwrap();
        open_daemon_log(&safe).unwrap().write_all(b"two").unwrap();
        assert_eq!(std::fs::read(&safe).unwrap(), b"onetwo");
        assert_eq!(std::fs::metadata(&safe).unwrap().mode() & 0o7777, 0o600);
    }

    #[test]
    fn given_new_daemon_state_should_create_private_directory_without_repairing_existing_state() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        // given
        let tmp = tempfile::TempDir::new().unwrap();
        let state = tmp.path().join("state");
        // when / then
        prepare_state_directory(&state).unwrap();
        assert_eq!(std::fs::metadata(&state).unwrap().mode() & 0o777, 0o700);
        std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(prepare_state_directory(&state).is_err());
        assert_eq!(std::fs::metadata(&state).unwrap().mode() & 0o777, 0o755);
        let alias = tmp.path().join("alias");
        std::os::unix::fs::symlink(&state, &alias).unwrap();
        assert!(prepare_state_directory(&alias).is_err());
    }

    #[test]
    fn given_find_daemon_binary_should_return_path_or_none() {
        let result = find_daemon_binary();
        // If found, verify the path is an executable file
        if let Some(path) = result {
            assert!(path.exists(), "found binary does not exist: {path:?}");
            assert!(
                path.to_string_lossy().contains("pu-engine"),
                "binary path should contain 'pu-engine': {path:?}"
            );
        }
        // If not found, that's fine — just verify no panic
    }

    #[tokio::test(flavor = "current_thread")]
    async fn given_running_daemon_should_report_healthy() {
        use std::os::unix::fs::PermissionsExt;
        use tempfile::TempDir;
        let tmp = TempDir::new().unwrap();
        std::fs::set_permissions(tmp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let sock = tmp.path().join("test.sock");

        let engine = pu_engine::engine::Engine::new();
        let server = pu_engine::ipc_server::IpcServer::bind(&sock, engine).unwrap();
        let handle = tokio::spawn(async move {
            server.run().await.ok();
        });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let healthy = check_daemon_health(&sock).await.unwrap();
        assert!(healthy);
        ensure_daemon(&sock).await.unwrap();

        crate::client::send_request(&sock, &pu_core::protocol::Request::Shutdown)
            .await
            .ok();
        handle.await.ok();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn given_no_daemon_should_report_not_healthy() {
        use tempfile::TempDir;
        let tmp = TempDir::new().unwrap();
        let sock = tmp.path().join("nope.sock");
        let healthy = check_daemon_health(&sock).await.unwrap();
        assert!(!healthy);
    }

    /// Serve one canned reply line (or none) to every connection on `sock`.
    fn fake_daemon(sock: &Path, reply: Option<String>) -> tokio::task::JoinHandle<()> {
        let listener = tokio::net::UnixListener::bind(sock).unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                let reply = reply.clone();
                tokio::spawn(async move {
                    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
                    let (reader, mut writer) = stream.split();
                    let mut line = String::new();
                    tokio::io::BufReader::new(reader)
                        .read_line(&mut line)
                        .await
                        .ok();
                    match reply {
                        Some(reply) => {
                            writer.write_all(reply.as_bytes()).await.ok();
                        }
                        None => std::future::pending::<()>().await,
                    }
                });
            }
        })
    }

    #[tokio::test(flavor = "current_thread")]
    async fn given_daemon_at_connection_limit_should_report_busy() {
        use tempfile::TempDir;
        let tmp = TempDir::new().unwrap();
        let sock = tmp.path().join("busy.sock");
        let server = fake_daemon(
            &sock,
            Some(
                "{\"type\":\"error\",\"code\":\"BUSY\",\"message\":\"daemon connection limit reached\"}\n".into(),
            ),
        );

        assert!(
            matches!(check_daemon_health(&sock).await, Err(CliError::DaemonError { code, .. }) if code == "BUSY")
        );
        server.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn given_incompatible_daemon_should_reject_without_stopping_it() {
        for version in [6, pu_core::protocol::PROTOCOL_VERSION + 1] {
            let tmp = tempfile::TempDir::new().unwrap();
            let sock = tmp.path().join("old.sock");
            let reply = serde_json::to_string(&Response::HealthReport {
                pid: 1,
                uptime_seconds: 0,
                protocol_version: version,
                projects: vec![],
                agent_count: 1,
            })
            .unwrap()
                + "\n";
            let server = fake_daemon(&sock, Some(reply));

            let error = ensure_daemon(&sock).await.unwrap_err().to_string();
            assert!(error.contains("protocol"), "{error}");
            assert!(error.contains("restart"), "{error}");
            let error = crate::commands::prompt::run_list(&sock, false)
                .await
                .unwrap_err()
                .to_string();
            assert!(error.contains("protocol"), "{error}");
            // The old daemon remains reachable, preserving its live sessions.
            assert!(
                matches!(crate::client::send_request(&sock, &Request::Health).await,
                Ok(Response::HealthReport { protocol_version, .. }) if protocol_version == version)
            );
            server.abort();
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn given_busy_daemon_should_not_assume_protocol_compatibility() {
        let tmp = tempfile::TempDir::new().unwrap();
        let sock = tmp.path().join("busy.sock");
        let server = fake_daemon(
            &sock,
            Some(
                "{\"type\":\"error\",\"code\":\"BUSY\",\"message\":\"daemon connection limit reached\"}\n".into(),
            ),
        );
        assert!(
            matches!(ensure_daemon(&sock).await, Err(CliError::DaemonError { code, .. }) if code == "BUSY")
        );
        server.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn given_daemon_that_never_answers_should_give_up_quickly() {
        use tempfile::TempDir;
        let tmp = TempDir::new().unwrap();
        let sock = tmp.path().join("mute.sock");
        let server = fake_daemon(&sock, None);

        let started = std::time::Instant::now();
        assert!(!check_daemon_health(&sock).await.unwrap());
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
        server.abort();
    }
}

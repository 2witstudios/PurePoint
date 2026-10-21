use std::path::{Path, PathBuf};

use pu_core::paths;
use pu_core::protocol::{PROTOCOL_VERSION, Request, Response};

use crate::error::CliError;

const DAEMON_BINARY: &str = "pu-engine";
/// Names the one `pu-engine` to start: a path, or a name looked up on `PATH`.
const ENGINE_PATH_ENV: &str = "PU_ENGINE_PATH";

/// The `pu-engine` this `pu` may auto-start. A `pu` inside an app bundle
/// starts only the bundle's own engine. Otherwise `PU_ENGINE_PATH` names the
/// only candidate when set; if not, the engine next to `pu` (where every
/// install puts both), then `~/.pu/bin`. `PATH` is used only through
/// `PU_ENGINE_PATH`. A candidate must be trusted (see [`is_trusted_binary`]).
pub fn find_daemon_binary() -> Option<PathBuf> {
    // Resolve symlinks so a linked `pu` still finds its real neighbors.
    let exe_dir = std::env::current_exe()
        .and_then(std::fs::canonicalize)
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf));
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let explicit = std::env::var_os(ENGINE_PATH_ENV).filter(|value| !value.is_empty());
    find_daemon_binary_in(exe_dir.as_deref(), home.as_deref(), explicit.as_deref())
}

/// `<x>.app/Contents` when `exe_dir` is that bundle's `Helpers` or `MacOS`.
fn bundle_contents(exe_dir: &Path) -> Option<&Path> {
    let name = exe_dir.file_name()?;
    let contents = exe_dir.parent()?;
    let is_bundle = (name == "Helpers" || name == "MacOS")
        && contents.file_name()? == "Contents"
        && contents.parent()?.extension()? == "app";
    is_bundle.then_some(contents)
}

/// Search order, without the trust check.
fn daemon_binary_candidates(
    exe_dir: Option<&Path>,
    home: Option<&Path>,
    explicit: Option<&std::ffi::OsStr>,
) -> Vec<PathBuf> {
    if let Some(contents) = exe_dir.and_then(bundle_contents) {
        return vec![contents.join("MacOS").join(DAEMON_BINARY)];
    }
    if let Some(explicit) = explicit {
        return which::which(explicit).into_iter().collect();
    }
    let mut candidates = Vec::new();
    if let Some(dir) = exe_dir {
        candidates.push(dir.join(DAEMON_BINARY));
    }
    if let Some(home) = home {
        candidates.push(home.join(".pu/bin").join(DAEMON_BINARY));
    }
    candidates
}

fn find_daemon_binary_in(
    exe_dir: Option<&Path>,
    home: Option<&Path>,
    explicit: Option<&std::ffi::OsStr>,
) -> Option<PathBuf> {
    daemon_binary_candidates(exe_dir, home, explicit)
        .into_iter()
        .find(|path| is_trusted_binary(path))
}

/// An executable file owned by this user or root that no other user can
/// modify: anyone who could rewrite it would run code as this user.
fn is_trusted_binary(path: &Path) -> bool {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    std::fs::metadata(path).is_ok_and(|meta| {
        // SAFETY: geteuid has no preconditions and cannot fail.
        let euid = unsafe { libc::geteuid() };
        meta.is_file() && trusted_mode_and_owner(meta.permissions().mode(), meta.uid(), euid)
    })
}

fn trusted_mode_and_owner(mode: u32, owner: u32, euid: u32) -> bool {
    mode & 0o111 != 0 && mode & 0o022 == 0 && (owner == euid || owner == 0)
}

/// Spawn the daemon detached from the caller's session: its own session (so
/// an SSH hangup or the caller's process group dying does not reach it) and no
/// inherited stdio (so `ssh host 'pu health'` returns instead of waiting on
/// the daemon's open descriptors).
fn daemon_command(
    binary: &Path,
    socket: &Path,
    stderr: std::process::Stdio,
) -> std::process::Command {
    use std::os::unix::process::CommandExt;
    let mut command = std::process::Command::new(binary);
    command
        .arg("--socket")
        .arg(socket)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(stderr);
    // SAFETY: setsid is async-signal-safe and touches no parent state.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    command
}

/// Start the installed supervisor's daemon for the default socket. Returns
/// false (spawn it directly) when no service is installed or the supervisor
/// cannot be reached, e.g. no systemd user bus in this session.
fn start_supervised(socket: &Path) -> bool {
    let is_default = paths::daemon_socket_path().is_ok_and(|default| default == socket);
    let Some(manager) = is_default.then(crate::service::installed).flatten() else {
        return false;
    };
    match crate::service::start(manager) {
        Ok(()) => true,
        Err(e) => {
            eprintln!(
                "warning: {} is installed but could not be started ({e}); starting pu-engine directly",
                manager.name()
            );
            false
        }
    }
}

/// Longest a single health probe may take. A daemon that accepts but never
/// answers would otherwise hold each probe for the full request timeout.
const HEALTH_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

/// What a single health probe learned about the daemon behind a socket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HealthProbe {
    Healthy,
    /// Nothing answered (no socket, refused, closed or timed out).
    NotAnswering,
    Incompatible {
        protocol_version: u32,
    },
    /// Alive, but at its connection limit; compatibility is unknown.
    Busy {
        message: String,
    },
}

pub async fn probe_daemon_health(socket: &Path) -> HealthProbe {
    let probe = crate::client::send_request(socket, &Request::Health);
    match tokio::time::timeout(HEALTH_PROBE_TIMEOUT, probe).await {
        Ok(Ok(Response::HealthReport {
            protocol_version, ..
        })) if protocol_version == PROTOCOL_VERSION => HealthProbe::Healthy,
        Ok(Ok(Response::HealthReport {
            protocol_version, ..
        })) => HealthProbe::Incompatible { protocol_version },
        Ok(Ok(Response::Error { code, message })) if code == "BUSY" => {
            HealthProbe::Busy { message }
        }
        _ => HealthProbe::NotAnswering,
    }
}

/// Check compatibility before any command; never restart a live daemon because
/// doing so would terminate its agent sessions.
pub async fn check_daemon_health(socket: &Path) -> Result<bool, CliError> {
    match probe_daemon_health(socket).await {
        HealthProbe::Healthy => Ok(true),
        HealthProbe::NotAnswering => Ok(false),
        HealthProbe::Incompatible { protocol_version } => Err(CliError::Other(format!(
            "daemon protocol v{protocol_version} is incompatible with CLI protocol v{PROTOCOL_VERSION} at {} — update pu and pu-engine together and restart the daemon after saving active agent work",
            socket.display()
        ))),
        // BUSY establishes liveness, but cannot establish protocol compatibility.
        HealthProbe::Busy { message } => Err(CliError::DaemonError {
            code: "BUSY".into(),
            message,
        }),
    }
}

/// Probe up to three times, 200 ms apart, so a busy or restarting daemon
/// that misses one check isn't reported as not answering.
pub async fn probe_daemon_health_with_retries(socket: &Path) -> HealthProbe {
    let mut probe = HealthProbe::NotAnswering;
    for attempt in 0..3 {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
        probe = probe_daemon_health(socket).await;
        if !matches!(probe, HealthProbe::NotAnswering) {
            break;
        }
    }
    probe
}

/// A machine's daemon is never auto-started (MACH-003): only check it.
pub async fn ensure_remote_daemon(socket: &Path, machine: &str) -> Result<(), CliError> {
    match probe_daemon_health_with_retries(socket).await {
        HealthProbe::Healthy => Ok(()),
        HealthProbe::NotAnswering => Err(CliError::Other(format!(
            "daemon not running on {machine}; start it with `pu machine start {machine}`"
        ))),
        HealthProbe::Incompatible { protocol_version } => Err(CliError::Other(format!(
            "daemon on {machine} speaks protocol v{protocol_version}, but this CLI speaks v{PROTOCOL_VERSION} — update PurePoint on both machines"
        ))),
        HealthProbe::Busy { message } => Err(CliError::DaemonError {
            code: "BUSY".into(),
            message: format!("daemon on {machine}: {message}"),
        }),
    }
}

pub async fn ensure_daemon(socket: &Path) -> Result<(), CliError> {
    if let Some(machine) = crate::machine_target::remote_machine() {
        return ensure_remote_daemon(socket, machine).await;
    }
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

    // A supervisor accepting the start does not establish that its daemon
    // became healthy: the supervised daemon can exit before answering (e.g. a
    // directly started daemon still holds the single-instance lock), so fall
    // back to the direct spawn before giving up.
    let supervised = start_supervised(socket);
    if !supervised {
        spawn_detached_daemon(socket)?;
    }
    if wait_for_healthy(socket).await?.is_some() {
        return Ok(());
    }
    if supervised {
        spawn_detached_daemon(socket)?;
        if wait_for_healthy(socket).await?.is_some() {
            return Ok(());
        }
    }
    Err(CliError::Other(
        "daemon did not start within 3 seconds".into(),
    ))
}

/// Poll health until the daemon answers or the budget runs out, with
/// exponential backoff: 10, 20, 40, 80, 160, 320, 640ms. The 3s budget is
/// wall-clock and includes the probes themselves, which can each take up to
/// HEALTH_PROBE_TIMEOUT against a daemon that never answers. Returns
/// `Ok(Some(()))` once healthy, `Ok(None)` if the budget ran out, and
/// propagates probe errors (incompatible protocol, BUSY) immediately.
async fn wait_for_healthy(socket: &Path) -> Result<Option<()>, CliError> {
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
    match tokio::time::timeout(std::time::Duration::from_secs(3), poll).await {
        Ok(result) => result.map(Some),
        Err(_) => Ok(None),
    }
}

fn spawn_detached_daemon(socket: &Path) -> Result<(), CliError> {
    let binary = find_daemon_binary().ok_or(CliError::Other(
        "no usable pu-engine next to pu or in ~/.pu/bin (or at PU_ENGINE_PATH) — install PurePoint (see docs/guide/linux-server.md) or `cargo install --path crates/pu-engine`".into(),
    ))?;

    // Redirect daemon stderr to log file so startup errors are diagnosable
    let stderr_target = match paths::daemon_log_path() {
        Ok(log_path) => {
            if let Some(parent) = log_path.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            match std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log_path)
            {
                Ok(file) => std::process::Stdio::from(file),
                Err(_) => std::process::Stdio::null(),
            }
        }
        Err(_) => std::process::Stdio::null(),
    };

    // Start daemon
    daemon_command(&binary, socket, stderr_target)
        .spawn()
        .map_err(CliError::Io)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn make_executable(path: &Path) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn given_daemon_beside_pu_should_prefer_it_over_install_dirs() {
        let tmp = tempfile::TempDir::new().unwrap();
        let exe_dir = tmp.path().join("bin");
        let home = tmp.path().join("home");
        make_executable(&exe_dir.join("pu-engine"));
        make_executable(&home.join(".pu/bin/pu-engine"));
        assert_eq!(
            find_daemon_binary_in(Some(&exe_dir), Some(&home), None),
            Some(exe_dir.join("pu-engine"))
        );
    }

    #[test]
    fn given_daemon_only_in_install_dirs_should_find_pu_bin_but_never_cargo_bin() {
        let tmp = tempfile::TempDir::new().unwrap();
        let exe_dir = tmp.path().join("empty");
        let home = tmp.path().join("home");
        make_executable(&home.join(".cargo/bin/pu-engine"));
        assert_eq!(
            find_daemon_binary_in(Some(&exe_dir), Some(&home), None),
            None
        );
        make_executable(&home.join(".pu/bin/pu-engine"));
        assert_eq!(
            find_daemon_binary_in(Some(&exe_dir), Some(&home), None),
            Some(home.join(".pu/bin/pu-engine"))
        );
    }

    #[test]
    fn given_pu_inside_app_bundle_should_start_only_the_bundled_engine() {
        let tmp = tempfile::TempDir::new().unwrap();
        let contents = tmp.path().join("PurePoint.app/Contents");
        let helpers = contents.join("Helpers");
        let home = tmp.path().join("home");
        make_executable(&helpers.join("pu-engine"));
        make_executable(&home.join(".pu/bin/pu-engine"));
        let path_engine = tmp.path().join("path/pu-engine");
        make_executable(&path_engine);
        let explicit = Some(path_engine.as_os_str());
        assert_eq!(
            find_daemon_binary_in(Some(&helpers), Some(&home), explicit),
            None,
            "a missing bundled engine must not fall back"
        );
        make_executable(&contents.join("MacOS/pu-engine"));
        for exe_dir in [&helpers, &contents.join("MacOS")] {
            assert_eq!(
                find_daemon_binary_in(Some(exe_dir), Some(&home), explicit),
                Some(contents.join("MacOS/pu-engine"))
            );
        }
    }

    #[test]
    fn given_explicit_engine_path_should_use_only_it() {
        let tmp = tempfile::TempDir::new().unwrap();
        let exe_dir = tmp.path().join("bin");
        make_executable(&exe_dir.join("pu-engine"));
        let dev = tmp.path().join("dev/pu-engine");
        make_executable(&dev);
        assert_eq!(
            find_daemon_binary_in(Some(&exe_dir), None, Some(dev.as_os_str())),
            Some(dev)
        );
        let missing = tmp.path().join("missing/pu-engine");
        assert_eq!(
            find_daemon_binary_in(Some(&exe_dir), None, Some(missing.as_os_str())),
            None
        );
    }

    #[test]
    fn given_engine_others_can_modify_should_refuse_it() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::TempDir::new().unwrap();
        let exe_dir = tmp.path().join("bin");
        let engine = exe_dir.join("pu-engine");
        make_executable(&engine);
        for shared in [0o775, 0o757, 0o777] {
            std::fs::set_permissions(&engine, std::fs::Permissions::from_mode(shared)).unwrap();
            assert_eq!(
                find_daemon_binary_in(Some(&exe_dir), None, None),
                None,
                "{shared:o}"
            );
        }
    }

    #[test]
    fn given_engine_owned_by_another_user_should_refuse_it() {
        assert!(trusted_mode_and_owner(0o755, 501, 501));
        assert!(trusted_mode_and_owner(0o755, 0, 501));
        assert!(!trusted_mode_and_owner(0o755, 502, 501));
        assert!(!trusted_mode_and_owner(0o644, 501, 501));
    }

    #[test]
    fn given_non_executable_candidate_should_skip_it() {
        let tmp = tempfile::TempDir::new().unwrap();
        let home = tmp.path().join("home");
        let path = home.join(".pu/bin/pu-engine");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "").unwrap();
        assert_eq!(find_daemon_binary_in(None, Some(&home), None), None);
    }

    #[test]
    fn given_daemon_spawn_should_detach_into_its_own_session() {
        // A stand-in daemon records its pid and process group; setsid makes it
        // the leader of a new session and process group (pgid == pid).
        let tmp = tempfile::TempDir::new().unwrap();
        let script = tmp.path().join("fake-engine");
        let out = tmp.path().join("out");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\necho $$ $(ps -o pgid= -p $$) > {}\n",
                out.display()
            ),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut child = daemon_command(
            &script,
            Path::new("/nonexistent.sock"),
            std::process::Stdio::null(),
        )
        .spawn()
        .unwrap();
        child.wait().unwrap();
        let report = std::fs::read_to_string(&out).unwrap();
        let ids: Vec<&str> = report.split_whitespace().collect();
        assert_eq!(ids.len(), 2, "{report:?}");
        assert_eq!(ids[0], ids[1], "daemon must lead its own process group");
        // SAFETY: getpgrp only reads process state.
        let own_pgid = unsafe { libc::getpgrp() };
        assert_ne!(ids[1], own_pgid.to_string());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn given_running_daemon_should_report_healthy() {
        use tempfile::TempDir;
        let tmp = TempDir::new().unwrap();
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
    async fn given_remote_endpoint_without_daemon_should_fail_without_starting_one() {
        let tmp = tempfile::TempDir::new().unwrap();
        let sock = tmp.path().join("forward.sock");
        let error = ensure_remote_daemon(&sock, "studio")
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("daemon not running on studio"), "{error}");
        assert!(error.contains("pu machine start studio"), "{error}");
        assert!(!sock.exists(), "nothing may bind the forwarded socket path");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn given_remote_daemon_with_other_protocol_should_report_incompatible() {
        let tmp = tempfile::TempDir::new().unwrap();
        let sock = tmp.path().join("old.sock");
        let reply = serde_json::to_string(&Response::HealthReport {
            pid: 1,
            uptime_seconds: 0,
            protocol_version: PROTOCOL_VERSION + 1,
            projects: vec![],
            agent_count: 0,
        })
        .unwrap()
            + "\n";
        let server = fake_daemon(&sock, Some(reply));
        let error = ensure_remote_daemon(&sock, "studio")
            .await
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("studio") && error.contains("protocol"),
            "{error}"
        );
        assert_eq!(
            probe_daemon_health(&sock).await,
            HealthProbe::Incompatible {
                protocol_version: PROTOCOL_VERSION + 1
            }
        );
        server.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn given_healthy_daemon_wait_for_healthy_should_return_some() {
        // given
        let tmp = tempfile::TempDir::new().unwrap();
        let sock = tmp.path().join("healthy.sock");
        let engine = pu_engine::engine::Engine::new();
        let server = pu_engine::ipc_server::IpcServer::bind(&sock, engine).unwrap();
        let handle = tokio::spawn(async move {
            server.run().await.ok();
        });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // when
        let result = wait_for_healthy(&sock).await;

        // then
        assert!(matches!(result, Ok(Some(()))), "{result:?}");
        crate::client::send_request(&sock, &Request::Shutdown)
            .await
            .ok();
        handle.await.ok();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn given_incompatible_daemon_wait_for_healthy_should_propagate_the_error() {
        // given
        let tmp = tempfile::TempDir::new().unwrap();
        let sock = tmp.path().join("old.sock");
        let reply = serde_json::to_string(&Response::HealthReport {
            pid: 1,
            uptime_seconds: 0,
            protocol_version: PROTOCOL_VERSION + 1,
            projects: vec![],
            agent_count: 0,
        })
        .unwrap()
            + "\n";
        let server = fake_daemon(&sock, Some(reply));

        // when
        let started = std::time::Instant::now();
        let result = wait_for_healthy(&sock).await;

        // then: the probe error surfaces immediately, never masked as a timeout
        let error = result.unwrap_err().to_string();
        assert!(error.contains("protocol"), "{error}");
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        server.abort();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn given_absent_daemon_wait_for_healthy_should_time_out_as_none() {
        // given: no listener at all, so every probe fails instantly
        let tmp = tempfile::TempDir::new().unwrap();
        let sock = tmp.path().join("absent.sock");

        // when
        let started = std::time::Instant::now();
        let result = wait_for_healthy(&sock).await;

        // then: the full 3s budget is spent before giving up, without an error
        assert!(matches!(result, Ok(None)), "{result:?}");
        assert!(started.elapsed() >= std::time::Duration::from_secs(3));
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

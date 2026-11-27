use std::path::Path;

use clap::Subcommand;
use pu_core::protocol::{Request, Response};

use crate::client;
use crate::commands::daemon_proxy;
use crate::daemon_ctrl;
use crate::error::CliError;
use crate::service::{self, Manager};

#[derive(Subcommand)]
pub enum DaemonAction {
    /// Start the daemon if it is not running (through its service when installed)
    Start,
    /// Stop the daemon; its agents are suspended and resume on next start
    Stop,
    /// Show daemon health and service state
    Status {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Run the daemon as a user service (systemd on Linux, launchd on macOS) so
    /// it restarts after a crash and survives logout; on Windows, register a
    /// logon Run entry (no admin rights needed)
    Install {
        /// Also start it now
        #[arg(long)]
        now: bool,
    },
    /// Remove the user service installed by `pu daemon install`
    Uninstall,
    /// Relay stdin/stdout to the daemon socket for SSH clients; never starts
    /// the daemon
    Proxy,
}

pub async fn run(socket: &Path, custom_socket: bool, action: DaemonAction) -> Result<(), CliError> {
    match action {
        DaemonAction::Start => {
            daemon_ctrl::ensure_daemon(socket).await?;
            println!("daemon running at {}", socket.display());
            Ok(())
        }
        DaemonAction::Stop => stop(socket, custom_socket).await,
        DaemonAction::Status { json } => status(socket, custom_socket, json).await,
        DaemonAction::Install { now } => {
            // A daemon started outside the service still holds the socket lock,
            // so the service's daemon would exit at once and stay inactive.
            let already_running = daemon_ctrl::check_daemon_health(socket)
                .await
                .unwrap_or(true);
            install(socket, custom_socket, now)?;
            // The Windows Run entry supervises nothing once started, so a
            // running daemon is already what it would start.
            if already_running && !cfg!(windows) {
                println!(
                    "a daemon is already running outside the service; run `pu daemon stop` then `pu daemon start` to move it under the service (running agents are suspended and resume)"
                );
            }
            Ok(())
        }
        DaemonAction::Uninstall => {
            require_default_socket(custom_socket)?;
            let manager = current_manager()?;
            match service::uninstall(manager)? {
                Some(file) => println!("removed {} ({})", manager.name(), file.display()),
                None => println!("no {} installed", manager.name()),
            }
            Ok(())
        }
        DaemonAction::Proxy => daemon_proxy::run(socket).await,
    }
}

fn current_manager() -> Result<Manager, CliError> {
    Manager::current()
        .ok_or_else(|| CliError::Other("no supported service manager on this platform".into()))
}

/// Services always supervise the default socket.
fn require_default_socket(custom_socket: bool) -> Result<(), CliError> {
    if custom_socket {
        return Err(CliError::Other(
            "the daemon service always uses the default socket (~/.pu/daemon.sock); drop --socket"
                .into(),
        ));
    }
    Ok(())
}

async fn stop(socket: &Path, custom_socket: bool) -> Result<(), CliError> {
    if !custom_socket
        && let Some(manager) = service::installed()
        && stop_supervised(manager, socket).await?
    {
        return Ok(());
    }
    if !daemon_ctrl::check_daemon_health(socket).await? {
        println!("daemon not running at {}", socket.display());
        return Ok(());
    }
    #[cfg(windows)]
    let pid = match client::send_request(socket, &Request::Health).await {
        Ok(Response::HealthReport { pid, .. }) => Some(pid),
        _ => None,
    };
    match client::send_request(socket, &Request::Shutdown).await? {
        Response::ShuttingDown | Response::Ok => {
            #[cfg(windows)]
            if let Some(pid) = pid
                && !win_exit::wait_for_exit(pid, win_exit::SHUTDOWN_BUDGET).await
            {
                eprintln!("warning: pu-engine (pid {pid}) is still shutting down");
            }
            println!("daemon stopped");
            Ok(())
        }
        Response::Error { code, message } => Err(CliError::DaemonError { code, message }),
        other => Err(CliError::Other(format!("unexpected response: {other:?}"))),
    }
}

/// Stop through the supervisor, verifying the socket actually went quiet:
/// `systemctl --user stop` exits 0 on an inactive unit even when a daemon
/// spawned directly still holds the socket. Returns false to fall through to
/// a direct `Shutdown`.
async fn stop_supervised(manager: Manager, socket: &Path) -> Result<bool, CliError> {
    match service::stop(manager) {
        Ok(true) if !daemon_ctrl::check_daemon_health(socket).await? => {
            println!("daemon stopped ({})", manager.name());
            Ok(true)
        }
        // launchd (Ok(false)), or systemd claimed success but a daemon that
        // was never under its control still answers the socket.
        Ok(_) => Ok(false),
        // The supervisor can be unreachable from this session (SSH without a
        // systemd user bus) while the daemon runs outside it; a direct
        // Shutdown still works there, mirroring `ensure_daemon`'s fallback.
        Err(e) => {
            eprintln!(
                "warning: {} could not be stopped ({e}); sending Shutdown directly",
                manager.name()
            );
            Ok(false)
        }
    }
}

async fn status(socket: &Path, custom_socket: bool, json: bool) -> Result<(), CliError> {
    let health = client::send_request(socket, &Request::Health).await;
    if json {
        println!("{}", status_json(socket, custom_socket, health));
        return Ok(());
    }
    match health {
        Ok(Response::HealthReport {
            pid,
            uptime_seconds,
            protocol_version,
            agent_count,
            ..
        }) => println!(
            "daemon running: pid {pid}, up {uptime_seconds}s, protocol v{protocol_version}, {agent_count} agent(s) at {}",
            socket.display()
        ),
        Ok(Response::Error { code, message }) => println!("daemon answered [{code}]: {message}"),
        _ => println!("daemon not running at {}", socket.display()),
    }
    if custom_socket {
        return Ok(());
    }
    match service::installed() {
        Some(manager) => println!("service: {} {}", manager.name(), service::state(manager)),
        None => println!("service: not installed (`pu daemon install --now` to supervise it)"),
    }
    Ok(())
}

/// `pu daemon status --json`: the daemon's health (or null when nothing
/// answers) and, for the default socket, the service manager's state.
fn status_json(
    socket: &Path,
    custom_socket: bool,
    health: Result<Response, CliError>,
) -> serde_json::Value {
    let daemon = match health {
        Ok(Response::HealthReport {
            pid,
            uptime_seconds,
            protocol_version,
            agent_count,
            ..
        }) => serde_json::json!({
            "running": true,
            "pid": pid,
            "uptime_seconds": uptime_seconds,
            "protocol_version": protocol_version,
            "agent_count": agent_count,
        }),
        Ok(Response::Error { code, message }) => serde_json::json!({
            "running": true,
            "error": { "code": code, "message": message },
        }),
        _ => serde_json::json!({ "running": false }),
    };
    let service = if custom_socket {
        serde_json::Value::Null
    } else {
        match service::installed() {
            Some(manager) => serde_json::json!({
                "installed": true,
                "manager": manager.name(),
                "state": service::state(manager),
            }),
            None => serde_json::json!({
                "installed": false,
                "manager": Manager::current().map(Manager::name),
            }),
        }
    };
    serde_json::json!({
        "socket": socket.display().to_string(),
        "daemon": daemon,
        "service": service,
    })
}

/// Waiting out the daemon's exit after `Shutdown` on Windows. The engine
/// answers first, then stops its agents' ConPTY trees (up to ~5s) while still
/// holding the single-instance lock, so a `pu daemon start` right after
/// `stop` spawned an engine that exited at once ("already running") and
/// reported that the daemon did not start.
#[cfg(windows)]
mod win_exit {
    use std::time::Duration;

    use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
    };

    /// The engine bounds its own shutdown at 5s; leave room for teardown.
    pub(super) const SHUTDOWN_BUDGET: Duration = Duration::from_secs(10);

    /// True once `pid` has exited (or never existed), false if it is still
    /// running after `budget`.
    pub(super) async fn wait_for_exit(pid: u32, budget: Duration) -> bool {
        let millis = u32::try_from(budget.as_millis()).unwrap_or(u32::MAX);
        tokio::task::spawn_blocking(move || {
            // SAFETY: plain handle open; a null result means the process is gone
            // (or not ours to wait on, which `Shutdown` already answered for).
            let process = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
            if process.is_null() {
                return true;
            }
            // SAFETY: `process` is a valid handle we own and close below.
            let waited = unsafe { WaitForSingleObject(process, millis) };
            // SAFETY: closing the handle opened above.
            unsafe { CloseHandle(process) };
            waited == WAIT_OBJECT_0
        })
        .await
        .unwrap_or(false)
    }
}

fn install(socket: &Path, custom_socket: bool, now: bool) -> Result<(), CliError> {
    require_default_socket(custom_socket)?;
    let manager = current_manager()?;
    let engine = daemon_ctrl::find_daemon_binary()
        .map_err(|reasons| CliError::Other(format!("no usable pu-engine ({reasons})")))?;
    // The service file outlives this shell; never record a relative path.
    let engine = pu_core::canon::canonicalize(&engine)?;
    for note in service::install(manager, &engine, socket, now)? {
        println!("{note}");
    }
    println!(
        "installed {} for {}{}",
        manager.name(),
        engine.display(),
        if now { " and started it" } else { "" }
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[tokio::test(flavor = "current_thread")]
    async fn given_running_process_should_wait_until_it_exits() {
        // given a process that exits on its own after about a second
        let mut child = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-Command", "Start-Sleep -Milliseconds 800"])
            .spawn()
            .expect("spawn child");
        let pid = child.id();

        // when / then: too short a budget reports it still running
        assert!(!win_exit::wait_for_exit(pid, std::time::Duration::from_millis(50)).await);
        // a long enough budget returns once it has exited
        assert!(win_exit::wait_for_exit(pid, std::time::Duration::from_secs(30)).await);
        assert!(child.try_wait().expect("try_wait").is_some());
    }

    #[cfg(windows)]
    #[tokio::test(flavor = "current_thread")]
    async fn given_exited_process_should_not_wait() {
        let mut child = std::process::Command::new("cmd.exe")
            .args(["/C", "exit 0"])
            .spawn()
            .expect("spawn child");
        let pid = child.id();
        child.wait().expect("wait");
        drop(child);

        let started = std::time::Instant::now();
        assert!(win_exit::wait_for_exit(pid, std::time::Duration::from_secs(30)).await);
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn given_custom_socket_should_reject_uninstall() {
        // given
        let dir = tempfile::tempdir().expect("tempdir");
        let socket = dir.path().join("custom.sock");

        // when
        let result = run(&socket, true, DaemonAction::Uninstall).await;

        // then
        match result {
            Err(CliError::Other(message)) => {
                assert!(
                    message.contains("default socket"),
                    "unexpected message: {message}"
                );
            }
            other => panic!("expected custom-socket rejection, got {other:?}"),
        }
    }
}

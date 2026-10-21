use std::path::Path;

use clap::Subcommand;
use pu_core::protocol::{Request, Response};

use crate::client;
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
    Status,
    /// Run the daemon as a user service (systemd on Linux, launchd on macOS) so
    /// it restarts after a crash and survives logout
    Install {
        /// Also start it now
        #[arg(long)]
        now: bool,
    },
    /// Remove the user service installed by `pu daemon install`
    Uninstall,
}

pub async fn run(socket: &Path, custom_socket: bool, action: DaemonAction) -> Result<(), CliError> {
    match action {
        DaemonAction::Start => {
            daemon_ctrl::ensure_daemon(socket).await?;
            println!("daemon running at {}", socket.display());
            Ok(())
        }
        DaemonAction::Stop => stop(socket, custom_socket).await,
        DaemonAction::Status => status(socket, custom_socket).await,
        DaemonAction::Install { now } => {
            // A daemon started outside the service still holds the socket lock,
            // so the service's daemon would exit at once and stay inactive.
            let already_running = daemon_ctrl::check_daemon_health(socket)
                .await
                .unwrap_or(true);
            install(socket, custom_socket, now)?;
            if already_running {
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
    match client::send_request(socket, &Request::Shutdown).await? {
        Response::ShuttingDown | Response::Ok => {
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

async fn status(socket: &Path, custom_socket: bool) -> Result<(), CliError> {
    match client::send_request(socket, &Request::Health).await {
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

fn install(socket: &Path, custom_socket: bool, now: bool) -> Result<(), CliError> {
    require_default_socket(custom_socket)?;
    let manager = current_manager()?;
    let engine = daemon_ctrl::find_daemon_binary()
        .map_err(|reasons| CliError::Other(format!("no usable pu-engine ({reasons})")))?;
    // The service file outlives this shell; never record a relative path.
    let engine = std::fs::canonicalize(&engine)?;
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

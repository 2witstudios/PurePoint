//! A machine's daemon over the system `ssh.exe` (WINAPP-013): one
//! `ssh.exe -T … <ssh_host> <relay>` process per daemon connection, whose stdio carries the
//! same NDJSON as the local pipe. `<relay>` runs `pu daemon proxy` on the machine; see
//! "Remote relay (Windows client)" in `docs/product/windows-app.md`.
//!
//! This is the transport [`super::client::DaemonClient`] needs for remote `git_exec` and
//! `fs_*` requests. Machine connection state, Start daemon and the relay's exit-code
//! reporting belong to `daemon/machine.rs` (WIN-8.14).

use std::ffi::OsString;
use std::path::PathBuf;
use std::pin::Pin;
use std::process::Stdio;
use std::sync::Mutex;
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::io::{AsyncRead, ReadBuf};
use tokio::process::{Child, ChildStdout, Command};

use super::client::{BoxRead, BoxWrite, Connector, DaemonError, OpenFuture};

/// The shell the relay runs in, detected once by probing `uname -s`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteShell {
    Unix,
    Windows,
}

impl RemoteShell {
    /// The remote command line `ssh.exe` passes to the machine.
    pub fn relay_command(self) -> &'static str {
        match self {
            Self::Unix => {
                r#"/bin/sh -c 'PATH="$HOME/.pu/bin:$HOME/.cargo/bin:$PATH"; export PATH; exec pu daemon proxy'"#
            }
            Self::Windows => "pu daemon proxy",
        }
    }
}

/// Exit status ssh itself uses when it cannot reach the host.
const SSH_CONNECT_FAILED: i32 = 255;
const PROBE_TIMEOUT: Duration = Duration::from_secs(20);

/// `ssh.exe`: `PU_SSH` when set, else Windows OpenSSH under `System32\OpenSSH`, else `PATH`.
pub fn find_ssh() -> Option<PathBuf> {
    if let Some(explicit) = std::env::var_os("PU_SSH").filter(|v| !v.is_empty()) {
        return which::which(explicit).ok();
    }
    let system = std::env::var_os("SystemRoot")
        .map(|root| PathBuf::from(root).join("System32").join("OpenSSH"));
    system
        .and_then(|dir| which::which_in("ssh", Some(dir), ".").ok())
        .or_else(|| which::which("ssh").ok())
}

/// The relay's common `ssh.exe` options (non-interactive, keep-alives).
fn ssh_args(host: &str) -> Vec<OsString> {
    [
        "-T",
        "-o",
        "BatchMode=yes",
        "-o",
        "ServerAliveInterval=15",
        "-o",
        "ServerAliveCountMax=3",
        host,
    ]
    .into_iter()
    .map(OsString::from)
    .collect()
}

fn command(ssh: &PathBuf) -> Command {
    let mut command = Command::new(ssh);
    // ssh.exe is a console program; the app has no console to lend it.
    #[cfg(windows)]
    command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
    command
}

/// Where a machine's daemon is: resolved on every connection, so registry edits apply.
#[derive(Debug, Clone)]
pub struct MachineTarget {
    pub ssh_host: String,
}

/// Resolves a machine (by id or name) to its SSH destination from `machines.json`.
pub fn machine_target(machine: &str) -> Result<MachineTarget, DaemonError> {
    let path = pu_core::paths::machines_registry_path()
        .map_err(|e| DaemonError::NotRunning(format!("No machine registry: {e}")))?;
    let registry = pu_core::machines::load(&path)
        .map_err(|e| DaemonError::NotRunning(format!("Could not read {}: {e}", path.display())))?;
    registry
        .machines
        .iter()
        .find(|m| m.id.eq_ignore_ascii_case(machine) || m.name.eq_ignore_ascii_case(machine))
        .map(|m| MachineTarget {
            ssh_host: m.ssh_host.clone(),
        })
        .ok_or_else(|| DaemonError::NotRunning(format!("Unknown machine {machine}")))
}

/// Opens daemon connections to one machine through `ssh.exe`.
pub struct SshRelayConnector {
    machine: String,
    shell: Mutex<Option<RemoteShell>>,
}

impl SshRelayConnector {
    pub fn new(machine: impl Into<String>) -> Self {
        Self {
            machine: machine.into(),
            shell: Mutex::new(None),
        }
    }

    /// `uname -s` printing something means a Unix shell; failing or printing nothing means
    /// Windows. Cached once ssh reached the host.
    async fn shell(&self, ssh: &PathBuf, host: &str) -> Result<RemoteShell, DaemonError> {
        if let Some(shell) = *self.shell.lock().unwrap_or_else(|p| p.into_inner()) {
            return Ok(shell);
        }
        let mut probe = command(ssh);
        probe
            .args(ssh_args(host))
            .arg("uname -s")
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let output = tokio::time::timeout(PROBE_TIMEOUT, probe.output())
            .await
            .map_err(|_| DaemonError::NotRunning(format!("Timed out reaching {host}")))?
            .map_err(|e| DaemonError::Io(format!("Could not run ssh: {e}")))?;
        if output.status.code() == Some(SSH_CONNECT_FAILED) {
            return Err(DaemonError::NotRunning(format!(
                "Could not reach {host} over SSH"
            )));
        }
        let shell = if output.status.success() && !output.stdout.trim_ascii().is_empty() {
            RemoteShell::Unix
        } else {
            RemoteShell::Windows
        };
        *self.shell.lock().unwrap_or_else(|p| p.into_inner()) = Some(shell);
        Ok(shell)
    }
}

impl Connector for SshRelayConnector {
    fn open(&self) -> OpenFuture<'_> {
        Box::pin(async move {
            let target = machine_target(&self.machine)?;
            let ssh = find_ssh().ok_or_else(|| {
                DaemonError::NotRunning("Could not find ssh.exe; install the OpenSSH client".into())
            })?;
            let shell = self.shell(&ssh, &target.ssh_host).await?;
            let mut relay = command(&ssh);
            relay
                .args(ssh_args(&target.ssh_host))
                .arg(shell.relay_command())
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .kill_on_drop(true);
            let mut child = relay
                .spawn()
                .map_err(|e| DaemonError::Io(format!("Could not run ssh: {e}")))?;
            let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
                return Err(DaemonError::Io("ssh has no stdio".into()));
            };
            let read = RelayRead {
                stdout,
                _child: child,
            };
            Ok((Box::new(read) as BoxRead, Box::new(stdin) as BoxWrite))
        })
    }

    fn describe(&self) -> String {
        format!("machine {}", self.machine)
    }
}

/// The relay's stdout. Owns the `ssh.exe` process, which ends when the connection drops.
struct RelayRead {
    stdout: ChildStdout,
    _child: Child,
}

impl AsyncRead for RelayRead {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.stdout).poll_read(cx, buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_each_shell_should_run_pu_daemon_proxy() {
        assert_eq!(RemoteShell::Windows.relay_command(), "pu daemon proxy");
        assert!(
            RemoteShell::Unix
                .relay_command()
                .ends_with("exec pu daemon proxy'")
        );
        let args: Vec<String> = ssh_args("me@box")
            .into_iter()
            .map(|a| a.into_string().unwrap())
            .collect();
        assert_eq!(
            args,
            [
                "-T",
                "-o",
                "BatchMode=yes",
                "-o",
                "ServerAliveInterval=15",
                "-o",
                "ServerAliveCountMax=3",
                "me@box"
            ]
        );
    }
}

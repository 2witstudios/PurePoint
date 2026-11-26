//! A machine's daemon over the system `ssh.exe` (WINAPP-013): one
//! `ssh.exe -T … <ssh_host> <relay>` process per daemon connection, whose stdio carries the
//! same NDJSON as the local pipe. `<relay>` runs `pu daemon proxy` on the machine; see
//! "Remote relay (Windows client)" in `docs/product/windows-app.md`.
//!
//! This is the transport [`super::client::DaemonClient`] uses for every remote request and
//! stream. The machine's health probe, Start daemon and the relay's exit-code reporting are
//! `daemon/machine.rs` (WIN-8.14); the argv builders here mirror `pu --machine`
//! (`crates/pu-cli/src/machine_target.rs`).

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

/// The Unix start command (`machines.md`): `pu health` starts the daemon beside `pu`.
pub const UNIX_START_COMMAND: &str =
    r#"/bin/sh -c 'PATH="$HOME/.pu/bin:$HOME/.cargo/bin:$PATH"; export PATH; exec pu health'"#;
/// The start command on a Windows machine.
pub const WINDOWS_START_COMMAND: &str = "pu health";
const OS_PROBE_COMMAND: &str = "uname -s";

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

    /// The command Start daemon runs on the machine (MACH-003: the only one that starts it).
    pub fn start_command(self) -> &'static str {
        match self {
            Self::Unix => UNIX_START_COMMAND,
            Self::Windows => WINDOWS_START_COMMAND,
        }
    }

    /// A machine is Windows when `uname -s` fails or prints nothing (cmd.exe and PowerShell
    /// have no `uname`), or names a Windows POSIX layer (an MSYS or Cygwin `uname` on the
    /// remote `PATH` still runs under cmd.exe). The CLI's `remote_os_from_probe`.
    pub fn from_probe(success: bool, stdout: &str) -> Self {
        let name = stdout.trim().to_ascii_uppercase();
        let windows_layer = ["MINGW", "MSYS", "CYGWIN"]
            .iter()
            .any(|prefix| name.starts_with(prefix));
        if !success || name.is_empty() || windows_layer {
            Self::Windows
        } else {
            Self::Unix
        }
    }
}

/// Exit status ssh itself uses when it cannot reach the host.
pub const SSH_CONNECT_FAILED: i32 = 255;
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

/// ssh destinations have no `host:port` form, so `user@host:port` becomes the
/// `ssh://user@host:port` URI. Aliases and bare hosts pass through unchanged
/// (`ssh_destination` in `crates/pu-cli/src/machine_target.rs`).
pub fn ssh_destination(host: &str) -> String {
    if host.starts_with("ssh://") {
        return host.to_string();
    }
    match host.rsplit_once(':') {
        Some((head, port))
            if !head.is_empty()
                && !head.contains(':')
                && !port.is_empty()
                && port.chars().all(|c| c.is_ascii_digit()) =>
        {
            format!("ssh://{host}")
        }
        _ => host.to_string(),
    }
}

/// Keeps a user's ControlMaster config from routing a call through (or parking it on) a
/// shared master: Win32-OpenSSH ignores it, but `PU_SSH` or `PATH` may name an ssh (Git for
/// Windows, MSYS2) that honours it.
const NO_CONTROL_MASTER: [&str; 4] = ["-o", "ControlMaster=no", "-o", "ControlPath=none"];

fn strings(args: &[&str]) -> Vec<OsString> {
    args.iter().map(OsString::from).collect()
}

/// `-T -o BatchMode=yes -o ServerAliveInterval=15 -o ServerAliveCountMax=3
/// -o ControlMaster=no -o ControlPath=none <destination>`: the relay's options.
pub fn ssh_args(host: &str) -> Vec<OsString> {
    let mut args = strings(&[
        "-T",
        "-o",
        "BatchMode=yes",
        "-o",
        "ServerAliveInterval=15",
        "-o",
        "ServerAliveCountMax=3",
    ]);
    args.extend(strings(&NO_CONTROL_MASTER));
    args.push(ssh_destination(host).into());
    args
}

/// `-T -o BatchMode=yes -o ControlMaster=no -o ControlPath=none <destination> 'uname -s'`.
pub fn os_probe_args(host: &str) -> Vec<OsString> {
    let mut args = strings(&["-T", "-o", "BatchMode=yes"]);
    args.extend(strings(&NO_CONTROL_MASTER));
    args.push(ssh_destination(host).into());
    args.push(OS_PROBE_COMMAND.into());
    args
}

/// `-o BatchMode=yes -o ControlMaster=no -o ControlPath=none <destination> <start command>`.
pub fn start_args(host: &str, shell: RemoteShell) -> Vec<OsString> {
    let mut args = strings(&["-o", "BatchMode=yes"]);
    args.extend(strings(&NO_CONTROL_MASTER));
    args.push(ssh_destination(host).into());
    args.push(shell.start_command().into());
    args
}

pub(crate) fn command(ssh: &PathBuf) -> Command {
    let mut command = Command::new(ssh);
    // ssh.exe is a console program; the app has no console to lend it.
    #[cfg(windows)]
    command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
    command
}

/// Where a machine's daemon is: resolved on every connection, so registry edits apply.
#[derive(Debug, Clone)]
pub struct MachineTarget {
    pub name: String,
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
            name: m.name.clone(),
            ssh_host: m.ssh_host.clone(),
        })
        .ok_or_else(|| DaemonError::NotRunning(format!("Unknown machine {machine}")))
}

/// The last non-empty line ssh wrote to stderr: its reason for failing.
pub fn last_line(stderr: &[u8]) -> Option<String> {
    String::from_utf8_lossy(stderr)
        .lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .map(str::to_owned)
}

/// Run the `uname -s` probe against `host`.
pub async fn probe_shell(ssh: &PathBuf, host: &str) -> Result<RemoteShell, DaemonError> {
    let mut probe = command(ssh);
    probe
        .args(os_probe_args(host))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let output = tokio::time::timeout(PROBE_TIMEOUT, probe.output())
        .await
        .map_err(|_| DaemonError::NotRunning(format!("Timed out reaching {host}")))?
        .map_err(|e| DaemonError::Io(format!("Could not run ssh: {e}")))?;
    if output.status.code() == Some(SSH_CONNECT_FAILED) {
        let reason =
            last_line(&output.stderr).unwrap_or_else(|| format!("Could not reach {host} over SSH"));
        return Err(DaemonError::NotRunning(reason));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(RemoteShell::from_probe(output.status.success(), &stdout))
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

    pub fn machine(&self) -> &str {
        &self.machine
    }

    /// The machine's shell, probed once ssh reached the host and cached from then on.
    pub async fn shell(&self, ssh: &PathBuf, host: &str) -> Result<RemoteShell, DaemonError> {
        if let Some(shell) = *self.shell.lock().unwrap_or_else(|p| p.into_inner()) {
            return Ok(shell);
        }
        let shell = probe_shell(ssh, host).await?;
        *self.shell.lock().unwrap_or_else(|p| p.into_inner()) = Some(shell);
        Ok(shell)
    }

    /// Forget the probed shell (the machine's host changed).
    pub fn reset(&self) {
        *self.shell.lock().unwrap_or_else(|p| p.into_inner()) = None;
    }

    /// Start one relay process: ssh with piped stdio and stderr captured as `stderr`.
    pub async fn spawn(&self, stderr: Stdio) -> Result<(Child, MachineTarget), DaemonError> {
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
            .stderr(stderr)
            .kill_on_drop(true);
        let child = relay
            .spawn()
            .map_err(|e| DaemonError::Io(format!("Could not run ssh: {e}")))?;
        Ok((child, target))
    }
}

impl Connector for SshRelayConnector {
    fn open(&self) -> OpenFuture<'_> {
        Box::pin(async move {
            let (mut child, _) = self.spawn(Stdio::null()).await?;
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

    fn text(args: Vec<OsString>) -> Vec<String> {
        args.into_iter().map(|a| a.into_string().unwrap()).collect()
    }

    #[test]
    fn given_each_shell_should_run_pu_daemon_proxy() {
        assert_eq!(RemoteShell::Windows.relay_command(), "pu daemon proxy");
        assert!(
            RemoteShell::Unix
                .relay_command()
                .ends_with("exec pu daemon proxy'")
        );
        assert_eq!(
            text(ssh_args("me@box")),
            [
                "-T",
                "-o",
                "BatchMode=yes",
                "-o",
                "ServerAliveInterval=15",
                "-o",
                "ServerAliveCountMax=3",
                "-o",
                "ControlMaster=no",
                "-o",
                "ControlPath=none",
                "me@box"
            ]
        );
    }

    #[test]
    fn given_host_with_port_should_use_the_ssh_uri_form() {
        // MachinesTests.givenHostWithPortShouldUseTheSSHURIForm
        assert_eq!(
            ssh_destination("jane@studio:2222"),
            "ssh://jane@studio:2222"
        );
        assert_eq!(ssh_destination("studio:22"), "ssh://studio:22");
        assert_eq!(ssh_destination("studio"), "studio");
        assert_eq!(ssh_destination("jane@studio"), "jane@studio");
        assert_eq!(
            ssh_destination("ssh://jane@studio:2222"),
            "ssh://jane@studio:2222"
        );
        assert_eq!(ssh_destination("studio:"), "studio:");
        assert_eq!(ssh_destination(":22"), ":22");
        assert_eq!(ssh_destination("studio:ssh"), "studio:ssh");
        assert_eq!(ssh_destination("fe80::1:22"), "fe80::1:22");
        // Every command uses the destination.
        let host = "jane@studio:2222";
        assert_eq!(
            text(ssh_args(host)).last().unwrap(),
            "ssh://jane@studio:2222"
        );
        let probe = text(os_probe_args(host));
        assert_eq!(probe[probe.len() - 2], "ssh://jane@studio:2222");
        let start = text(start_args(host, RemoteShell::Unix));
        assert_eq!(start[start.len() - 2], "ssh://jane@studio:2222");
    }

    #[test]
    fn given_start_daemon_should_put_installed_locations_on_path() {
        // MachinesTests.givenStartDaemonShouldPutInstalledLocationsOnPath
        assert_eq!(
            text(start_args("studio", RemoteShell::Unix)),
            [
                "-o",
                "BatchMode=yes",
                "-o",
                "ControlMaster=no",
                "-o",
                "ControlPath=none",
                "studio",
                r#"/bin/sh -c 'PATH="$HOME/.pu/bin:$HOME/.cargo/bin:$PATH"; export PATH; exec pu health'"#,
            ]
        );
        assert_eq!(
            text(start_args("studio", RemoteShell::Windows))
                .last()
                .unwrap(),
            "pu health"
        );
        assert_eq!(
            text(os_probe_args("studio")),
            [
                "-T",
                "-o",
                "BatchMode=yes",
                "-o",
                "ControlMaster=no",
                "-o",
                "ControlPath=none",
                "studio",
                "uname -s"
            ]
        );
    }

    #[test]
    fn given_uname_output_should_tell_unix_from_windows() {
        assert_eq!(RemoteShell::from_probe(true, "Linux\n"), RemoteShell::Unix);
        assert_eq!(RemoteShell::from_probe(true, "Darwin"), RemoteShell::Unix);
        assert_eq!(RemoteShell::from_probe(false, ""), RemoteShell::Windows);
        assert_eq!(RemoteShell::from_probe(true, "  "), RemoteShell::Windows);
        assert_eq!(
            RemoteShell::from_probe(true, "MINGW64_NT-10.0"),
            RemoteShell::Windows
        );
        assert_eq!(
            RemoteShell::from_probe(true, "CYGWIN_NT"),
            RemoteShell::Windows
        );
    }

    #[test]
    fn given_ssh_stderr_should_keep_its_last_line() {
        assert_eq!(
            last_line(b"warning: x\r\nssh: Could not resolve hostname box\r\n\r\n").as_deref(),
            Some("ssh: Could not resolve hostname box")
        );
        assert_eq!(last_line(b"  \n"), None);
    }
}

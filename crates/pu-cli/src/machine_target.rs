//! `--machine <name>`: reach a registered machine's daemon over SSH.
//!
//! The CLI reuses the app's live forward (`~/.pu/machines/<id>/daemon.sock`)
//! when it answers; otherwise it opens a private `ssh -N -L` forward on a
//! per-invocation socket and closes it when the command finishes, so the two
//! never unlink each other's socket. A machine's daemon is never auto-started.
//!
//! `PU_SSH` overrides the ssh binary (default `/usr/bin/ssh`), e.g. for tests.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use pu_core::machines::{self, Machine};
use pu_core::paths;

use crate::daemon_ctrl::{self, HealthProbe};
use crate::error::CliError;

pub const SSH_ENV: &str = "PU_SSH";
const DEFAULT_SSH: &str = "/usr/bin/ssh";
const FORWARD_READY_TIMEOUT: Duration = Duration::from_secs(10);
const HOME_QUERY_TIMEOUT: Duration = Duration::from_secs(20);
/// `sun_path` holds 104 bytes on macOS, including the terminating NUL.
const MAX_SOCKET_PATH_LEN: usize = 103;

/// Remote `PATH` is minimal for non-interactive SSH, so try the install
/// locations before falling back to `PATH`.
pub const START_COMMAND: &str =
    r#""$HOME/.pu/bin/pu" health || "$HOME/.cargo/bin/pu" health || pu health"#;

static REMOTE_MACHINE: OnceLock<String> = OnceLock::new();

/// Mark this invocation as targeting a machine (disables auto-start and
/// local project fallbacks).
pub fn set_remote_machine(name: String) {
    let _ = REMOTE_MACHINE.set(name);
}

pub fn remote_machine() -> Option<&'static str> {
    REMOTE_MACHINE.get().map(String::as_str)
}

pub fn is_remote() -> bool {
    remote_machine().is_some()
}

pub fn ssh_program() -> OsString {
    std::env::var_os(SSH_ENV)
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| DEFAULT_SSH.into())
}

/// ssh destinations have no `host:port` form; `user@host:port` needs the
/// `ssh://` URI form. Aliases and bare hosts pass through unchanged.
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

pub fn home_query_args(host: &str) -> Vec<OsString> {
    vec![
        "-o".into(),
        "BatchMode=yes".into(),
        ssh_destination(host).into(),
        r#"printf %s "$HOME""#.into(),
    ]
}

pub fn forward_args(local_socket: &Path, remote_home: &str, host: &str) -> Vec<OsString> {
    let mut spec = local_socket.as_os_str().to_os_string();
    spec.push(":");
    spec.push(remote_daemon_socket(remote_home));
    let mut args: Vec<OsString> = vec!["-N".into(), "-T".into()];
    for option in [
        "BatchMode=yes",
        "ExitOnForwardFailure=yes",
        "ServerAliveInterval=15",
        "ServerAliveCountMax=3",
        "StreamLocalBindUnlink=yes",
    ] {
        args.push("-o".into());
        args.push(option.into());
    }
    args.push("-L".into());
    // One argv element, so a remote home with spaces survives intact.
    args.push(spec);
    args.push(ssh_destination(host).into());
    args
}

pub fn start_args(host: &str) -> Vec<OsString> {
    vec![ssh_destination(host).into(), START_COMMAND.into()]
}

fn remote_daemon_socket(remote_home: &str) -> String {
    format!("{}/.pu/daemon.sock", remote_home.trim_end_matches('/'))
}

pub fn core_error(e: pu_core::error::PuError) -> CliError {
    CliError::Other(e.to_string())
}

pub fn load_registry() -> Result<machines::MachineRegistry, CliError> {
    machines::load(&paths::machines_registry_path()?).map_err(core_error)
}

pub fn find_machine(name: &str) -> Result<Machine, CliError> {
    load_registry()?.find(name).cloned().ok_or_else(|| {
        CliError::Other(format!(
            "unknown machine {name:?} — see `pu machine list`, or register it with `pu machine add {name} <ssh_host>`"
        ))
    })
}

fn unreachable(machine: &Machine, detail: impl std::fmt::Display) -> CliError {
    CliError::Other(format!(
        "machine {} ({}) is unreachable over ssh: {detail}",
        machine.name, machine.ssh_host
    ))
}

/// Resolve the remote user's `$HOME` (once per connection).
pub async fn resolve_remote_home(machine: &Machine) -> Result<String, CliError> {
    machines::validate_ssh_host(&machine.ssh_host).map_err(core_error)?;
    let program = ssh_program();
    let run = tokio::process::Command::new(&program)
        .args(home_query_args(&machine.ssh_host))
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true)
        .output();
    let output = tokio::time::timeout(HOME_QUERY_TIMEOUT, run)
        .await
        .map_err(|_| unreachable(machine, "timed out"))?
        .map_err(|e| {
            CliError::Other(format!(
                "failed to run {}: {e}",
                Path::new(&program).display()
            ))
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = match stderr.trim() {
            "" => output.status.to_string(),
            message => message.to_string(),
        };
        return Err(unreachable(machine, detail));
    }
    let home = String::from_utf8_lossy(&output.stdout)
        .trim_end_matches(['\r', '\n'])
        .to_string();
    if !home.starts_with('/') {
        return Err(unreachable(
            machine,
            format!("remote $HOME is not an absolute path: {home:?}"),
        ));
    }
    Ok(home)
}

/// A private ssh forward; dropping it stops ssh and removes its socket.
pub struct Forward {
    child: Option<std::process::Child>,
    socket: PathBuf,
}

impl Forward {
    pub fn socket(&self) -> &Path {
        &self.socket
    }
}

impl Drop for Forward {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = std::fs::remove_file(&self.socket);
    }
}

/// Where a command reaches a machine's daemon.
pub enum Endpoint {
    /// The app's live forward.
    App(PathBuf),
    Private(Forward),
}

impl Endpoint {
    pub fn socket(&self) -> &Path {
        match self {
            Endpoint::App(path) => path,
            Endpoint::Private(forward) => forward.socket(),
        }
    }
}

fn pid_is_alive(pid: libc::pid_t) -> bool {
    // SAFETY: signal 0 only checks for existence and permission.
    let result = unsafe { libc::kill(pid, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}

/// Remove per-invocation sockets left by CLI processes that died without
/// cleaning up (e.g. killed by a signal).
pub fn sweep_stale_sockets(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(pid) = name
            .to_str()
            .and_then(|n| n.strip_prefix("cli-"))
            .and_then(|n| n.strip_suffix(".sock"))
            .and_then(|n| n.parse::<libc::pid_t>().ok())
        else {
            continue;
        };
        if pid > 0 && !pid_is_alive(pid) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Open a forward for this invocation and wait until its socket is bound.
pub async fn open_forward(machine: &Machine) -> Result<Forward, CliError> {
    let dir = machines::ensure_machine_dir(&machine.id).map_err(core_error)?;
    sweep_stale_sockets(&dir);
    let socket = dir.join(format!("cli-{}.sock", std::process::id()));
    if socket.as_os_str().len() > MAX_SOCKET_PATH_LEN {
        return Err(CliError::Other(format!(
            "socket path {} is too long for a Unix socket (max {MAX_SOCKET_PATH_LEN} bytes)",
            socket.display()
        )));
    }
    let _ = std::fs::remove_file(&socket);
    let home = resolve_remote_home(machine).await?;

    let program = ssh_program();
    let child = std::process::Command::new(&program)
        .args(forward_args(&socket, &home, &machine.ssh_host))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| {
            CliError::Other(format!(
                "failed to run {}: {e}",
                Path::new(&program).display()
            ))
        })?;
    let mut forward = Forward {
        child: Some(child),
        socket,
    };

    let deadline = Instant::now() + FORWARD_READY_TIMEOUT;
    loop {
        // symlink_metadata: the socket exists once bound, whatever it points at.
        if forward.socket.symlink_metadata().is_ok() {
            return Ok(forward);
        }
        let child = forward.child.as_mut().expect("forward child");
        if let Some(status) = child.try_wait()? {
            let mut stderr = String::new();
            if let Some(mut pipe) = child.stderr.take() {
                use std::io::Read;
                let _ = pipe.read_to_string(&mut stderr);
            }
            let detail = match stderr.trim() {
                "" => format!("ssh exited ({status}) before the forward was ready"),
                message => message.to_string(),
            };
            return Err(unreachable(machine, detail));
        }
        if Instant::now() >= deadline {
            return Err(unreachable(
                machine,
                format!(
                    "forward was not ready within {}s",
                    FORWARD_READY_TIMEOUT.as_secs()
                ),
            ));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Reach `machine`: the app's live forward when it answers, else a private one.
/// An app forward whose daemon is incompatible or busy is reported, not bypassed.
pub async fn connect(machine: &Machine) -> Result<Endpoint, CliError> {
    let app_socket = paths::machine_socket_path(&machine.id)?;
    if app_socket.symlink_metadata().is_ok() {
        match daemon_ctrl::probe_daemon_health(&app_socket).await {
            HealthProbe::NotAnswering => {}
            _ => return Ok(Endpoint::App(app_socket)),
        }
    }
    Ok(Endpoint::Private(open_forward(machine).await?))
}

/// The daemon state `pu machine status` reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MachineState {
    Running,
    Busy,
    Stopped,
    Incompatible { protocol_version: u32 },
    Unreachable { detail: String },
}

impl MachineState {
    pub fn label(&self) -> &'static str {
        match self {
            MachineState::Running => "running",
            MachineState::Busy => "busy",
            MachineState::Stopped => "stopped",
            MachineState::Incompatible { .. } => "incompatible",
            MachineState::Unreachable { .. } => "unreachable",
        }
    }

    pub fn detail(&self) -> Option<String> {
        match self {
            MachineState::Incompatible { protocol_version } => Some(format!(
                "daemon protocol v{protocol_version}, CLI protocol v{}",
                pu_core::protocol::PROTOCOL_VERSION
            )),
            MachineState::Unreachable { detail } => Some(detail.clone()),
            _ => None,
        }
    }
}

/// Probe a machine's daemon without ever starting it.
pub async fn probe(machine: &Machine) -> MachineState {
    let endpoint = match connect(machine).await {
        Ok(endpoint) => endpoint,
        Err(e) => {
            return MachineState::Unreachable {
                detail: e.to_string(),
            };
        }
    };
    for attempt in 0..3 {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        match daemon_ctrl::probe_daemon_health(endpoint.socket()).await {
            HealthProbe::Healthy => return MachineState::Running,
            HealthProbe::Busy { .. } => return MachineState::Busy,
            HealthProbe::Incompatible { protocol_version } => {
                return MachineState::Incompatible { protocol_version };
            }
            HealthProbe::NotAnswering => {}
        }
    }
    MachineState::Stopped
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(args: Vec<OsString>) -> Vec<String> {
        args.into_iter().map(|a| a.into_string().unwrap()).collect()
    }

    #[test]
    fn given_forward_should_match_documented_ssh_invocation() {
        let args = forward_args(
            Path::new("/Users/me/.pu/machines/id/cli-1.sock"),
            "/home/My User/",
            "studio",
        );
        assert_eq!(
            strings(args),
            vec![
                "-N",
                "-T",
                "-o",
                "BatchMode=yes",
                "-o",
                "ExitOnForwardFailure=yes",
                "-o",
                "ServerAliveInterval=15",
                "-o",
                "ServerAliveCountMax=3",
                "-o",
                "StreamLocalBindUnlink=yes",
                "-L",
                "/Users/me/.pu/machines/id/cli-1.sock:/home/My User/.pu/daemon.sock",
                "studio",
            ]
        );
    }

    #[test]
    fn given_home_query_should_run_batch_mode_printf() {
        assert_eq!(
            strings(home_query_args("me@box")),
            vec!["-o", "BatchMode=yes", "me@box", r#"printf %s "$HOME""#]
        );
    }

    #[test]
    fn given_start_should_try_install_locations_in_one_remote_command() {
        assert_eq!(
            strings(start_args("studio")),
            vec![
                "studio",
                r#""$HOME/.pu/bin/pu" health || "$HOME/.cargo/bin/pu" health || pu health"#
            ]
        );
    }

    #[test]
    fn given_host_with_port_should_use_ssh_uri() {
        assert_eq!(ssh_destination("me@box:2222"), "ssh://me@box:2222");
        assert_eq!(ssh_destination("box:22"), "ssh://box:22");
        assert_eq!(ssh_destination("studio"), "studio");
        assert_eq!(ssh_destination("me@box"), "me@box");
        assert_eq!(ssh_destination("ssh://me@box:2"), "ssh://me@box:2");
        assert_eq!(ssh_destination("fe80::1"), "fe80::1");
    }

    #[test]
    fn given_dead_cli_socket_should_be_swept_but_live_ones_kept() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mine = tmp.path().join(format!("cli-{}.sock", std::process::id()));
        // Reap a short-lived child so its pid is known to be dead.
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let dead_pid = child.id();
        child.wait().unwrap();
        let dead = tmp.path().join(format!("cli-{dead_pid}.sock"));
        let app = tmp.path().join("daemon.sock");
        for path in [&mine, &dead, &app] {
            std::fs::write(path, "").unwrap();
        }
        sweep_stale_sockets(tmp.path());
        assert!(mine.exists());
        assert!(app.exists());
        assert!(!dead.exists());
    }

    #[test]
    fn given_states_should_have_stable_labels() {
        assert_eq!(MachineState::Running.label(), "running");
        assert_eq!(MachineState::Stopped.label(), "stopped");
        assert_eq!(
            MachineState::Incompatible {
                protocol_version: 1
            }
            .label(),
            "incompatible"
        );
        assert_eq!(
            MachineState::Unreachable { detail: "x".into() }
                .detail()
                .as_deref(),
            Some("x")
        );
    }
}

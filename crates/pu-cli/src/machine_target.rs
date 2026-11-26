//! `--machine <name>`: reach a registered machine's daemon over SSH.
//!
//! The CLI reuses the app's live forward (`~/.pu/machines/<id>/daemon.sock`)
//! when it answers; otherwise it opens a private `ssh -N -L` forward on a
//! per-invocation socket and closes it when the command finishes, so the two
//! never unlink each other's socket. A machine's daemon is never auto-started.
//!
//! On Windows there is no forward: each daemon connection is one `ssh.exe`
//! exec relay running `pu daemon proxy` on the machine (MACH-012), with the
//! relay command chosen by a `uname -s` probe once per connection.
//!
//! `PU_SSH` overrides the ssh binary (default `/usr/bin/ssh`; on Windows the
//! `ssh` found on `PATH`, normally `System32\OpenSSH\ssh.exe`), e.g. for tests.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
#[cfg(unix)]
use std::time::Instant;

use pu_core::machines::{self, Machine};
use pu_core::paths;

use crate::daemon_ctrl::{self, HealthProbe};
use crate::error::CliError;

pub const SSH_ENV: &str = "PU_SSH";
#[cfg(unix)]
const DEFAULT_SSH: &str = "/usr/bin/ssh";
#[cfg(unix)]
const FORWARD_READY_TIMEOUT: Duration = Duration::from_secs(10);
const HOME_QUERY_TIMEOUT: Duration = Duration::from_secs(20);
/// `sun_path` holds 104 bytes on macOS, including the terminating NUL.
#[cfg(unix)]
const MAX_SOCKET_PATH_LEN: usize = 103;

/// Remote `PATH` is minimal for non-interactive SSH, so put the install
/// locations on it to find `pu`. `pu` then starts the `pu-engine` beside
/// itself (or in `~/.pu/bin`), never one found on `PATH`.
/// Run through `/bin/sh` so it also works when the login shell is fish or csh.
pub const START_COMMAND: &str =
    r#"/bin/sh -c 'PATH="$HOME/.pu/bin:$HOME/.cargo/bin:$PATH"; export PATH; exec pu health'"#;

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
        .unwrap_or_else(default_ssh)
}

#[cfg(unix)]
fn default_ssh() -> OsString {
    DEFAULT_SSH.into()
}

/// The `ssh` on `PATH` (Win32-OpenSSH ships in `System32\OpenSSH`, which is
/// on the default `PATH`), else that install location itself.
#[cfg(windows)]
fn default_ssh() -> OsString {
    which::which("ssh").map(Into::into).unwrap_or_else(|_| {
        let root = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
        PathBuf::from(root)
            .join("System32")
            .join("OpenSSH")
            .join("ssh.exe")
            .into()
    })
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

/// Keeps a user's ControlMaster config from routing a command through (or
/// parking it on) a shared master connection.
const NO_CONTROL_MASTER: [&str; 4] = ["-o", "ControlMaster=no", "-o", "ControlPath=none"];

#[cfg(unix)]
pub fn home_query_args(host: &str) -> Vec<OsString> {
    let mut args: Vec<OsString> = vec!["-o".into(), "BatchMode=yes".into()];
    args.extend(NO_CONTROL_MASTER.map(OsString::from));
    args.extend([ssh_destination(host).into(), r#"printf %s "$HOME""#.into()]);
    args
}

#[cfg(unix)]
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
        // A user's ControlMaster config would otherwise hand the forward to a
        // shared master that outlives this ssh process.
        "ControlMaster=no",
        "ControlPath=none",
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
    let mut args: Vec<OsString> = NO_CONTROL_MASTER.map(OsString::from).into();
    args.extend([ssh_destination(host).into(), START_COMMAND.into()]);
    args
}

#[cfg(unix)]
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
#[cfg(unix)]
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
#[cfg(unix)]
pub struct Forward {
    child: Option<std::process::Child>,
    socket: PathBuf,
}

#[cfg(unix)]
impl Forward {
    pub fn socket(&self) -> &Path {
        &self.socket
    }
}

/// Private forwards that are open right now (ssh pid, socket), so a
/// termination signal can close them: `Drop` doesn't run on signals.
#[cfg(unix)]
static OPEN_FORWARDS: Mutex<Vec<(u32, PathBuf)>> = Mutex::new(Vec::new());

#[cfg(unix)]
fn open_forwards() -> std::sync::MutexGuard<'static, Vec<(u32, PathBuf)>> {
    OPEN_FORWARDS.lock().unwrap_or_else(|e| e.into_inner())
}

/// Stop every open private forward's ssh and remove its socket and pid file.
#[cfg(unix)]
pub fn close_open_forwards() {
    for (pid, socket) in open_forwards().drain(..) {
        if let Ok(pid) = libc::pid_t::try_from(pid) {
            // SAFETY: plain signal delivery to the ssh child this process spawned.
            unsafe {
                libc::kill(pid, libc::SIGKILL);
            }
        }
        let _ = pu_core::fs_retry::remove_file(&socket);
        let _ = pu_core::fs_retry::remove_file(pid_file_path(&socket));
    }
}

/// On SIGTERM, SIGHUP or SIGINT, close open private forwards and exit with
/// the conventional `128 + signal` status.
#[cfg(unix)]
pub fn close_forwards_on_signal() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::signal::unix::{SignalKind, signal};
    // Installed once, before the first forward starts waiting to be ready.
    static INSTALLED: AtomicBool = AtomicBool::new(false);
    if INSTALLED.swap(true, Ordering::SeqCst) {
        return;
    }
    let (Ok(mut term), Ok(mut hup), Ok(mut int)) = (
        signal(SignalKind::terminate()),
        signal(SignalKind::hangup()),
        signal(SignalKind::interrupt()),
    ) else {
        return;
    };
    tokio::spawn(async move {
        let number = tokio::select! {
            _ = term.recv() => libc::SIGTERM,
            _ = hup.recv() => libc::SIGHUP,
            _ = int.recv() => libc::SIGINT,
        };
        close_open_forwards();
        std::process::exit(128 + number);
    });
}

#[cfg(unix)]
impl Drop for Forward {
    fn drop(&mut self) {
        open_forwards().retain(|(_, socket)| socket != &self.socket);
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = pu_core::fs_retry::remove_file(&self.socket);
        let _ = pu_core::fs_retry::remove_file(pid_file_path(&self.socket));
    }
}

/// `<sock>.pid`: the ssh pid behind a private forward, so a sweep can stop an
/// ssh orphaned by a CLI that died without cleaning up.
#[cfg(unix)]
fn pid_file_path(socket: &Path) -> PathBuf {
    let mut path = socket.as_os_str().to_os_string();
    path.push(".pid");
    PathBuf::from(path)
}

/// Where a command reaches a machine's daemon.
pub enum Endpoint {
    /// The app's live forward.
    #[cfg(unix)]
    App(PathBuf),
    #[cfg(unix)]
    Private(Forward),
    /// An ssh exec relay per daemon connection (MACH-012).
    #[cfg(windows)]
    Relay(RelayEndpoint),
}

impl Endpoint {
    /// The path commands pass as their socket. On Windows it names no file:
    /// it keys the relay `client` opens for each daemon connection.
    pub fn socket(&self) -> &Path {
        match self {
            #[cfg(unix)]
            Endpoint::App(path) => path,
            #[cfg(unix)]
            Endpoint::Private(forward) => forward.socket(),
            #[cfg(windows)]
            Endpoint::Relay(relay) => &relay.key,
        }
    }
}

#[cfg(unix)]
fn pid_is_alive(pid: libc::pid_t) -> bool {
    // SAFETY: signal 0 only checks for existence and permission.
    let result = unsafe { libc::kill(pid, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}

/// `ps -p <pid> -o <field>=` (`-ww`: never truncate), if the pid exists.
#[cfg(unix)]
fn ps_field(pid: libc::pid_t, field: &str) -> Option<String> {
    let output = std::process::Command::new("ps")
        .args(["-ww", "-p", &pid.to_string(), "-o", &format!("{field}=")])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Whether `pid` is the ssh forward for `socket`: its executable is named
/// `ssh` (`ps` reports a path on macOS and a bare name on Linux) and its
/// arguments carry `-L <socket>:`. Guards against signalling a recycled pid.
#[cfg(unix)]
fn pid_is_forward_ssh(pid: libc::pid_t, socket: &Path) -> bool {
    let is_ssh = ps_field(pid, "comm")
        .is_some_and(|comm| Path::new(&comm).file_name().is_some_and(|n| n == "ssh"));
    is_ssh
        && ps_field(pid, "args")
            .is_some_and(|args| args.contains(&format!("-L {}:", socket.display())))
}

/// The owning CLI pid of `cli-<pid>.sock` or `cli-<pid>.sock.pid`.
#[cfg(unix)]
fn cli_owner_pid(name: &str) -> Option<libc::pid_t> {
    let rest = name.strip_prefix("cli-")?;
    let rest = rest
        .strip_suffix(".sock.pid")
        .or_else(|| rest.strip_suffix(".sock"))?;
    rest.parse::<libc::pid_t>().ok().filter(|pid| *pid > 0)
}

/// Clean up after CLI processes that died without doing so (e.g. killed by a
/// signal): stop their recorded ssh forward, then remove its socket and pid file.
#[cfg(unix)]
pub fn sweep_stale_sockets(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(owner) = name.to_str().and_then(cli_owner_pid) else {
            continue;
        };
        if pid_is_alive(owner) {
            continue;
        }
        let path = entry.path();
        if name.to_str().is_some_and(|n| n.ends_with(".pid")) {
            let socket = dir.join(format!("cli-{owner}.sock"));
            let ssh_pid = std::fs::read_to_string(&path)
                .ok()
                .and_then(|s| s.trim().parse::<libc::pid_t>().ok())
                .filter(|pid| *pid > 0);
            if let Some(ssh_pid) = ssh_pid
                && pid_is_alive(ssh_pid)
                && pid_is_forward_ssh(ssh_pid, &socket)
            {
                // SAFETY: plain signal delivery to a verified ssh process.
                unsafe {
                    libc::kill(ssh_pid, libc::SIGTERM);
                }
            }
        }
        let _ = pu_core::fs_retry::remove_file(&path);
    }
}

/// Open a forward for this invocation and wait until its socket is bound.
#[cfg(unix)]
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
    let _ = pu_core::fs_retry::remove_file(&socket);
    let home = resolve_remote_home(machine).await?;
    // A signal while ssh is still coming up must close it too.
    close_forwards_on_signal();

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
    // Best effort: without it a crash leaves ssh running, as before.
    let _ = std::fs::write(pid_file_path(&socket), child.id().to_string());
    open_forwards().push((child.id(), socket.clone()));
    let mut forward = Forward {
        child: Some(child),
        socket,
    };

    let deadline = Instant::now() + FORWARD_READY_TIMEOUT;
    loop {
        // symlink_metadata: the socket exists once bound, whatever it points at.
        if forward.socket.symlink_metadata().is_ok() {
            // ssh logs a line per failed channel (a stopped remote daemon, say);
            // an unread pipe would fill and stall the forward.
            if let Some(mut pipe) = forward.child.as_mut().and_then(|c| c.stderr.take()) {
                std::thread::spawn(move || {
                    let _ = std::io::copy(&mut pipe, &mut std::io::sink());
                });
            }
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
#[cfg(unix)]
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

/// The ssh command `pu machine start` runs: the start command (MACH-003
/// still holds: only this command ever starts a machine's daemon).
#[cfg(unix)]
pub async fn start_command_args(machine: &Machine) -> Result<Vec<OsString>, CliError> {
    Ok(start_args(&machine.ssh_host))
}

/// What a machine's sshd runs commands under, from the `uname -s` probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(unix, allow(dead_code))]
pub enum RemoteOs {
    Unix,
    Windows,
}

/// The relay on a Unix machine: the iPad's exec string (MACH-009), wrapped
/// like the start command so `pu` resolves under any login shell.
#[cfg_attr(unix, allow(dead_code))]
pub const UNIX_RELAY_COMMAND: &str = r#"/bin/sh -c 'PATH="$HOME/.pu/bin:$HOME/.cargo/bin:$PATH"; export PATH; exec pu daemon proxy'"#;
/// The relay on a Windows machine: sshd builds the session from the user's
/// registry `PATH`, which includes `%USERPROFILE%\.pu\bin`.
#[cfg_attr(unix, allow(dead_code))]
pub const WINDOWS_RELAY_COMMAND: &str = "pu daemon proxy";
/// The start command on a Windows machine.
#[cfg_attr(unix, allow(dead_code))]
pub const WINDOWS_START_COMMAND: &str = "pu health";
#[cfg_attr(unix, allow(dead_code))]
const OS_PROBE_COMMAND: &str = "uname -s";
/// ssh's own failure status (connection, auth, host key), as opposed to the
/// remote command's.
#[cfg_attr(unix, allow(dead_code))]
const SSH_FAILED: i32 = 255;

/// `ssh -T -o BatchMode=yes -o ServerAliveInterval=15 -o ServerAliveCountMax=3
/// -o ControlMaster=no -o ControlPath=none <ssh_host> <relay>` (machines.md,
/// Windows relay). Win32-OpenSSH has no connection sharing, but the `ssh` on
/// `PATH` may be another build (Git for Windows, MSYS2) honouring a user's
/// ControlMaster config, which would hand the relay to a shared master.
#[cfg_attr(unix, allow(dead_code))]
pub fn relay_args(host: &str, os: RemoteOs) -> Vec<OsString> {
    let relay = match os {
        RemoteOs::Unix => UNIX_RELAY_COMMAND,
        RemoteOs::Windows => WINDOWS_RELAY_COMMAND,
    };
    let mut args: Vec<OsString> = vec!["-T".into()];
    for option in [
        "BatchMode=yes",
        "ServerAliveInterval=15",
        "ServerAliveCountMax=3",
    ] {
        args.push("-o".into());
        args.push(option.into());
    }
    args.extend(NO_CONTROL_MASTER.map(OsString::from));
    args.extend([ssh_destination(host).into(), relay.into()]);
    args
}

/// `ssh -T -o BatchMode=yes <ssh_host> 'uname -s'`, run once per connection.
#[cfg_attr(unix, allow(dead_code))]
pub fn os_probe_args(host: &str) -> Vec<OsString> {
    let mut args: Vec<OsString> = vec!["-T".into(), "-o".into(), "BatchMode=yes".into()];
    args.extend(NO_CONTROL_MASTER.map(OsString::from));
    args.extend([ssh_destination(host).into(), OS_PROBE_COMMAND.into()]);
    args
}

/// A machine is Windows when `uname -s` fails or prints nothing (cmd.exe and
/// PowerShell have no `uname`), or names a Windows POSIX layer (an MSYS or
/// Cygwin `uname` on the remote `PATH` still runs under cmd.exe).
#[cfg_attr(unix, allow(dead_code))]
pub fn remote_os_from_probe(success: bool, stdout: &str) -> RemoteOs {
    let name = stdout.trim().to_ascii_uppercase();
    let windows_layer = ["MINGW", "MSYS", "CYGWIN"]
        .iter()
        .any(|prefix| name.starts_with(prefix));
    if !success || name.is_empty() || windows_layer {
        RemoteOs::Windows
    } else {
        RemoteOs::Unix
    }
}

/// `pu machine start` for a machine of kind `os`: the Unix start command, or
/// `pu health` on a Windows machine.
#[cfg_attr(unix, allow(dead_code))]
pub fn start_args_for(host: &str, os: RemoteOs) -> Vec<OsString> {
    match os {
        RemoteOs::Unix => start_args(host),
        RemoteOs::Windows => {
            let mut args: Vec<OsString> = NO_CONTROL_MASTER.map(OsString::from).into();
            args.extend([ssh_destination(host).into(), WINDOWS_START_COMMAND.into()]);
            args
        }
    }
}

/// Why a relay closed without a reply, from its exit status: 69 is no
/// daemon, 127 or 9009 is no `pu`, 255 is ssh itself failing.
#[cfg_attr(unix, allow(dead_code))]
pub fn relay_exit_error(machine: &Machine, code: Option<i32>, stderr: &str) -> CliError {
    let stderr = stderr.trim();
    match code {
        Some(crate::commands::daemon_proxy::EXIT_NO_DAEMON) => CliError::DaemonNotRunning,
        Some(127 | 9009) => CliError::Other(format!(
            "PurePoint is not installed on machine {} (`pu` was not found there) — install it there and retry",
            machine.name
        )),
        Some(SSH_FAILED) => unreachable(
            machine,
            if stderr.is_empty() {
                "ssh exited with status 255"
            } else {
                stderr
            },
        ),
        _ => {
            let status = code.map_or("no exit status".to_string(), |c| format!("exit {c}"));
            let detail = if stderr.is_empty() {
                status
            } else {
                format!("{status}: {stderr}")
            };
            CliError::Other(format!(
                "the daemon relay on machine {} ({}) closed without a reply ({detail})",
                machine.name, machine.ssh_host
            ))
        }
    }
}

/// How long a relayed health check may take: it includes the ssh handshake.
#[cfg(windows)]
pub const RELAY_HEALTH_TIMEOUT: Duration = Duration::from_secs(20);
/// How long a relay that stopped talking gets to report its exit status.
#[cfg(windows)]
const RELAY_EXIT_TIMEOUT: Duration = Duration::from_secs(5);

/// One machine's relay target, registered for the life of a connection.
#[cfg(windows)]
#[derive(Debug, Clone)]
pub struct Relay {
    machine: Machine,
    os: RemoteOs,
}

#[cfg(windows)]
static RELAYS: Mutex<Vec<(PathBuf, Relay)>> = Mutex::new(Vec::new());

#[cfg(windows)]
fn relays() -> std::sync::MutexGuard<'static, Vec<(PathBuf, Relay)>> {
    RELAYS.lock().unwrap_or_else(|e| e.into_inner())
}

/// A machine connection on Windows: while it lives, daemon connections to
/// its key path run the relay. Nothing is bound or written to disk.
#[cfg(windows)]
pub struct RelayEndpoint {
    key: PathBuf,
}

#[cfg(windows)]
impl Drop for RelayEndpoint {
    fn drop(&mut self) {
        let mut relays = relays();
        if let Some(index) = relays.iter().position(|(key, _)| key == &self.key) {
            relays.remove(index);
        }
    }
}

/// The relay behind `socket`, if it is a machine connection's key.
#[cfg(windows)]
pub fn relay_for(socket: &Path) -> Option<Relay> {
    relays()
        .iter()
        .rev()
        .find(|(key, _)| key == socket)
        .map(|(_, relay)| relay.clone())
}

/// Run the `uname -s` probe; only ssh's own failure makes a machine unreachable.
#[cfg(windows)]
pub async fn probe_remote_os(machine: &Machine) -> Result<RemoteOs, CliError> {
    machines::validate_ssh_host(&machine.ssh_host).map_err(core_error)?;
    let program = ssh_program();
    let run = tokio::process::Command::new(&program)
        .args(os_probe_args(&machine.ssh_host))
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
    if output.status.code() == Some(SSH_FAILED) {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = match stderr.trim() {
            "" => output.status.to_string(),
            message => message.to_string(),
        };
        return Err(unreachable(machine, detail));
    }
    Ok(remote_os_from_probe(
        output.status.success(),
        &String::from_utf8_lossy(&output.stdout),
    ))
}

/// Reach `machine`: probe its OS once, then relay every daemon connection.
#[cfg(windows)]
pub async fn connect(machine: &Machine) -> Result<Endpoint, CliError> {
    let os = probe_remote_os(machine).await?;
    let key = paths::machine_socket_path(&machine.id)?;
    relays().push((
        key.clone(),
        Relay {
            machine: machine.clone(),
            os,
        },
    ));
    Ok(Endpoint::Relay(RelayEndpoint { key }))
}

/// The ssh command `pu machine start` runs, after probing the machine's OS.
#[cfg(windows)]
pub async fn start_command_args(machine: &Machine) -> Result<Vec<OsString>, CliError> {
    let os = probe_remote_os(machine).await?;
    Ok(start_args_for(&machine.ssh_host, os))
}

/// One running relay: NDJSON goes to `writer` and comes back on `reader`.
/// Dropping it kills the ssh process.
#[cfg(windows)]
pub struct RelayConnection {
    pub reader: tokio::process::ChildStdout,
    pub writer: tokio::process::ChildStdin,
    pub exit: RelayExit,
}

/// The relay's ssh process and its collected stderr.
#[cfg(windows)]
pub struct RelayExit {
    child: tokio::process::Child,
    stderr: tokio::sync::oneshot::Receiver<Vec<u8>>,
    machine: Machine,
}

#[cfg(windows)]
impl Relay {
    /// Start `ssh <host> <relay>` for one daemon connection.
    pub fn open(&self) -> Result<RelayConnection, CliError> {
        let program = ssh_program();
        let mut child = tokio::process::Command::new(&program)
            .args(relay_args(&self.machine.ssh_host, self.os))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| {
                CliError::Other(format!(
                    "failed to run {}: {e}",
                    Path::new(&program).display()
                ))
            })?;
        let (Some(reader), Some(writer), Some(stderr)) =
            (child.stdout.take(), child.stdin.take(), child.stderr.take())
        else {
            return Err(CliError::Other("ssh relay stdio was not piped".into()));
        };
        // Drained concurrently so a chatty ssh never stalls on a full pipe, on
        // a detached thread: a read the runtime owned would hold up the CLI's
        // exit while anything (a relay's own child) keeps the pipe open.
        let mut stderr = std::fs::File::from(stderr.into_owned_handle()?);
        let (done, collected) = tokio::sync::oneshot::channel();
        std::thread::spawn(move || {
            use std::io::Read;
            let mut bytes = Vec::new();
            let _ = stderr.read_to_end(&mut bytes);
            let _ = done.send(bytes);
        });
        let stderr = collected;
        Ok(RelayConnection {
            reader,
            writer,
            exit: RelayExit {
                child,
                stderr,
                machine: self.machine.clone(),
            },
        })
    }
}

#[cfg(windows)]
impl RelayExit {
    /// The error for a relay that closed without a reply.
    pub async fn failure(mut self) -> CliError {
        let code = match tokio::time::timeout(RELAY_EXIT_TIMEOUT, self.child.wait()).await {
            Ok(Ok(status)) => status.code(),
            _ => {
                let _ = self.child.kill().await;
                None
            }
        };
        let stderr = match tokio::time::timeout(Duration::from_secs(1), &mut self.stderr).await {
            Ok(Ok(bytes)) => String::from_utf8_lossy(&bytes).into_owned(),
            _ => String::new(),
        };
        relay_exit_error(&self.machine, code, &stderr)
    }
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
    match daemon_ctrl::probe_daemon_health_with_retries(endpoint.socket()).await {
        HealthProbe::Healthy => MachineState::Running,
        HealthProbe::Busy { .. } => MachineState::Busy,
        HealthProbe::Incompatible { protocol_version } => {
            MachineState::Incompatible { protocol_version }
        }
        #[cfg(unix)]
        HealthProbe::NotAnswering => MachineState::Stopped,
        // Ask the relay once more for why: no daemon (exit 69) is "stopped",
        // while a missing `pu` or a failed ssh session is not.
        #[cfg(windows)]
        HealthProbe::NotAnswering => {
            let health =
                crate::client::send_request(endpoint.socket(), &pu_core::protocol::Request::Health);
            match tokio::time::timeout(RELAY_HEALTH_TIMEOUT, health).await {
                Ok(Err(CliError::DaemonNotRunning)) | Ok(Ok(_)) => MachineState::Stopped,
                Ok(Err(e)) => MachineState::Unreachable {
                    detail: e.to_string(),
                },
                Err(_) => MachineState::Unreachable {
                    detail: format!("no reply within {}s", RELAY_HEALTH_TIMEOUT.as_secs()),
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(args: Vec<OsString>) -> Vec<String> {
        args.into_iter().map(|a| a.into_string().unwrap()).collect()
    }

    #[cfg(unix)]
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
                "-o",
                "ControlMaster=no",
                "-o",
                "ControlPath=none",
                "-L",
                "/Users/me/.pu/machines/id/cli-1.sock:/home/My User/.pu/daemon.sock",
                "studio",
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn given_home_query_should_run_batch_mode_printf() {
        assert_eq!(
            strings(home_query_args("me@box")),
            vec![
                "-o",
                "BatchMode=yes",
                "-o",
                "ControlMaster=no",
                "-o",
                "ControlPath=none",
                "me@box",
                r#"printf %s "$HOME""#
            ]
        );
    }

    #[test]
    fn given_start_should_put_install_locations_on_remote_path() {
        assert_eq!(
            strings(start_args("studio")),
            vec![
                "-o",
                "ControlMaster=no",
                "-o",
                "ControlPath=none",
                "studio",
                r#"/bin/sh -c 'PATH="$HOME/.pu/bin:$HOME/.cargo/bin:$PATH"; export PATH; exec pu health'"#
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

    #[cfg(unix)]
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

    #[cfg(unix)]
    #[test]
    fn given_cli_file_names_should_parse_owner_pid() {
        assert_eq!(cli_owner_pid("cli-42.sock"), Some(42));
        assert_eq!(cli_owner_pid("cli-42.sock.pid"), Some(42));
        assert_eq!(cli_owner_pid("daemon.sock"), None);
        assert_eq!(cli_owner_pid("cli-0.sock"), None);
        assert_eq!(cli_owner_pid("cli-x.sock.pid"), None);
    }

    #[cfg(unix)]
    fn dead_pid() -> u32 {
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        pid
    }

    /// A long-running process whose executable is named `ssh` (a copy of
    /// bash), with `-L <forward>` in its arguments when given.
    #[cfg(unix)]
    fn spawn_fake_ssh(dir: &Path, forward: Option<&Path>) -> std::process::Child {
        let fake = dir.join("ssh");
        if !fake.exists() {
            std::fs::copy(which::which("bash").unwrap(), &fake).unwrap();
        }
        let mut command = std::process::Command::new(&fake);
        // `bash -c <script> <$0> <$1>`: the trailing args are inert.
        command.args(["-c", "while :; do sleep 0.1; done"]);
        if let Some(socket) = forward {
            let mut spec = socket.as_os_str().to_os_string();
            spec.push(":/home/me/.pu/daemon.sock");
            command.arg("-L").arg(spec);
        }
        // On Linux a just-copied executable can be briefly "text file busy" while
        // another test thread's fork still holds the copy's write descriptor.
        for _ in 0..100 {
            match command.spawn() {
                Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                result => return result.unwrap(),
            }
        }
        command.spawn().unwrap()
    }

    #[cfg(unix)]
    fn wait_exit(child: &mut std::process::Child) -> Option<std::process::ExitStatus> {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if let Some(status) = child.try_wait().unwrap() {
                return Some(status);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        None
    }

    #[cfg(unix)]
    #[test]
    fn given_dead_cli_with_recorded_ssh_should_stop_ssh_and_remove_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        let bin = tmp.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let owner = dead_pid();
        let sock = tmp.path().join(format!("cli-{owner}.sock"));
        let mut ssh = spawn_fake_ssh(&bin, Some(&sock));
        let pid_file = pid_file_path(&sock);
        std::fs::write(&sock, "").unwrap();
        std::fs::write(&pid_file, ssh.id().to_string()).unwrap();

        sweep_stale_sockets(tmp.path());

        let status = wait_exit(&mut ssh);
        if status.is_none() {
            let _ = ssh.kill();
        }
        assert!(status.is_some(), "orphaned ssh forward should be stopped");
        assert!(!sock.exists());
        assert!(!pid_file.exists());
    }

    #[cfg(unix)]
    #[test]
    fn given_recorded_pid_that_is_not_ssh_should_not_signal_it() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut other = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let owner = dead_pid();
        let pid_file = tmp.path().join(format!("cli-{owner}.sock.pid"));
        std::fs::write(&pid_file, other.id().to_string()).unwrap();

        sweep_stale_sockets(tmp.path());

        assert!(
            other.try_wait().unwrap().is_none(),
            "non-ssh pid was signalled"
        );
        assert!(!pid_file.exists());
        other.kill().unwrap();
        other.wait().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn given_recorded_ssh_for_another_forward_should_not_signal_it() {
        let tmp = tempfile::TempDir::new().unwrap();
        let bin = tmp.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let owner = dead_pid();
        let sock = tmp.path().join(format!("cli-{owner}.sock"));
        let pid_file = pid_file_path(&sock);
        for forward in [None, Some(tmp.path().join("cli-1.sock"))] {
            let mut ssh = spawn_fake_ssh(&bin, forward.as_deref());
            std::fs::write(&pid_file, ssh.id().to_string()).unwrap();

            sweep_stale_sockets(tmp.path());

            assert!(
                ssh.try_wait().unwrap().is_none(),
                "ssh without `-L {}:` was signalled",
                sock.display()
            );
            assert!(!pid_file.exists());
            ssh.kill().unwrap();
            ssh.wait().unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    fn given_live_cli_should_keep_its_ssh_and_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        let bin = tmp.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let sock = tmp.path().join(format!("cli-{}.sock", std::process::id()));
        let mut ssh = spawn_fake_ssh(&bin, Some(&sock));
        let pid_file = pid_file_path(&sock);
        std::fs::write(&sock, "").unwrap();
        std::fs::write(&pid_file, ssh.id().to_string()).unwrap();

        sweep_stale_sockets(tmp.path());

        assert!(ssh.try_wait().unwrap().is_none());
        assert!(sock.exists() && pid_file.exists());
        ssh.kill().unwrap();
        ssh.wait().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn given_open_forward_should_be_closed_by_signal_cleanup() {
        let tmp = tempfile::TempDir::new().unwrap();
        let bin = tmp.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let sock = tmp.path().join("cli-1.sock");
        let mut ssh = spawn_fake_ssh(&bin, Some(&sock));
        std::fs::write(&sock, "").unwrap();
        std::fs::write(pid_file_path(&sock), ssh.id().to_string()).unwrap();
        open_forwards().push((ssh.id(), sock.clone()));

        close_open_forwards();

        let status = wait_exit(&mut ssh);
        if status.is_none() {
            let _ = ssh.kill();
        }
        assert!(status.is_some(), "forward ssh survived the signal cleanup");
        assert!(!sock.exists() && !pid_file_path(&sock).exists());
        assert!(!open_forwards().iter().any(|(_, s)| s == &sock));
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

    #[test]
    fn given_unix_machine_should_relay_through_the_wrapped_proxy_command() {
        assert_eq!(
            strings(relay_args("me@box:2222", RemoteOs::Unix)),
            vec![
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
                "ssh://me@box:2222",
                r#"/bin/sh -c 'PATH="$HOME/.pu/bin:$HOME/.cargo/bin:$PATH"; export PATH; exec pu daemon proxy'"#,
            ]
        );
    }

    #[test]
    fn given_windows_machine_should_relay_through_bare_pu_daemon_proxy() {
        assert_eq!(
            strings(relay_args("winbox", RemoteOs::Windows)),
            vec![
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
                "winbox",
                "pu daemon proxy",
            ]
        );
    }

    #[test]
    fn given_os_probe_should_run_batch_mode_uname() {
        assert_eq!(
            strings(os_probe_args("studio")),
            vec![
                "-T",
                "-o",
                "BatchMode=yes",
                "-o",
                "ControlMaster=no",
                "-o",
                "ControlPath=none",
                "studio",
                "uname -s",
            ]
        );
    }

    #[test]
    fn given_uname_result_should_pick_the_remote_os() {
        assert_eq!(remote_os_from_probe(true, "Linux\n"), RemoteOs::Unix);
        assert_eq!(remote_os_from_probe(true, "Darwin\r\n"), RemoteOs::Unix);
        assert_eq!(remote_os_from_probe(false, ""), RemoteOs::Windows);
        assert_eq!(remote_os_from_probe(false, "Linux"), RemoteOs::Windows);
        assert_eq!(remote_os_from_probe(true, "  \r\n"), RemoteOs::Windows);
        assert_eq!(
            remote_os_from_probe(true, "MINGW64_NT-10.0-26100\n"),
            RemoteOs::Windows
        );
    }

    #[test]
    fn given_machine_kind_should_pick_the_start_command() {
        assert_eq!(
            strings(start_args_for("studio", RemoteOs::Unix)),
            strings(start_args("studio"))
        );
        assert_eq!(
            strings(start_args_for("winbox", RemoteOs::Windows)),
            vec![
                "-o",
                "ControlMaster=no",
                "-o",
                "ControlPath=none",
                "winbox",
                "pu health"
            ]
        );
    }

    #[test]
    fn given_relay_exit_status_should_explain_the_failure() {
        let machine = Machine {
            id: "id".into(),
            name: "box".into(),
            ssh_host: "me@box".into(),
        };
        assert!(matches!(
            relay_exit_error(&machine, Some(69), "error: no daemon"),
            CliError::DaemonNotRunning
        ));
        for code in [127, 9009] {
            let message = relay_exit_error(&machine, Some(code), "").to_string();
            assert!(
                message.contains("not installed on machine box"),
                "{message}"
            );
        }
        let message = relay_exit_error(&machine, Some(255), "Permission denied\r\n").to_string();
        assert!(
            message.contains("unreachable over ssh: Permission denied"),
            "{message}"
        );
        let message = relay_exit_error(&machine, Some(1), "boom").to_string();
        assert!(message.contains("exit 1: boom"), "{message}");
    }
}

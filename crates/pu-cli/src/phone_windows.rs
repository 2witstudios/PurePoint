//! `pu phone` on Windows: the Point Guard bridge as a per-user logon task.
//!
//! - Autostart is the `HKCU\...\CurrentVersion\Run` value `PurePoint Point
//!   Guard` (no elevation, no service), whose command is `"<pu.exe>" phone
//!   start`. That `pu` derives the payload from its own version, so the
//!   payload and `pu` stay in lockstep as on Linux.
//! - `start` launches the payload's `node.exe bridge\main.js --managed`
//!   detached and without a console, with an explicit environment, and
//!   records its pid and creation time in `~\.pu\pointguard\phone-host.json`
//!   (a recycled pid never matches).
//! - State lives in `%LOCALAPPDATA%\PurePoint\point-guard`, where the bridge
//!   keeps it; the owner talks to it over the token-guarded owner pipe.
//! - The bridge discovers the tailnet itself (`POINT_GUARD_DISCOVER_TAILNET`),
//!   binding only the address `tailscale ip -4` and an interface agree on;
//!   without Tailscale it does not listen, and `status` says so.
//!
//! Every outside effect (registry, Tailscale, processes) goes through an
//! adapter, so tests run against temp homes with fakes.

use std::ffi::OsString;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::error::CliError;
use crate::phone::{self, Admin};

/// The Run value that starts Point Guard at logon.
pub const RUN_VALUE: &str = "PurePoint Point Guard";
const RECORD: &str = "phone-host.json";
const LOG: &str = "phone-host.log";
pub const NODE_BIN: &str = r"bin\node.exe";
pub const LOCK_BIN: &str = r"bin\point-guard-lock.exe";
pub const BRIDGE_ENTRY: &str = r"bridge\main.js";

/// Where the logon command is registered.
pub trait Autostart {
    fn read(&self) -> io::Result<Option<String>>;
    fn write(&self, command: &str) -> io::Result<()>;
    /// False when there was nothing to delete.
    fn delete(&self) -> io::Result<bool>;
    /// Turned off in Settings > Apps > Startup.
    fn disabled(&self) -> bool;
    fn display(&self) -> String;
}

/// The real `HKCU` Run value, through `service.rs`'s registry manager.
pub struct RunKey;

impl Autostart for RunKey {
    fn read(&self) -> io::Result<Option<String>> {
        crate::service::run_key::read_value(RUN_VALUE)
    }
    fn write(&self, command: &str) -> io::Result<()> {
        crate::service::run_key::write_value(RUN_VALUE, command)
    }
    fn delete(&self) -> io::Result<bool> {
        crate::service::run_key::delete_value(RUN_VALUE)
    }
    fn disabled(&self) -> bool {
        crate::service::run_key::value_disabled_in_startup_apps(RUN_VALUE)
    }
    fn display(&self) -> String {
        format!(r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run\{RUN_VALUE}")
    }
}

/// What `tailscale ip -4` says about this PC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tailnet {
    /// No `tailscale.exe` at a standard location.
    Missing,
    /// Installed, but no single tailnet IPv4 (logged out, stopped, ambiguous).
    Down,
    Address(String),
}

pub trait Tailscale {
    fn probe(&self) -> Tailnet;
}

/// The installed Tailscale CLI, at fixed locations only (never `PATH`).
pub struct TailscaleCli {
    pub candidates: Vec<PathBuf>,
}

impl TailscaleCli {
    pub fn standard() -> Self {
        let mut candidates = Vec::new();
        if let Some(dir) = std::env::var_os("ProgramFiles")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
        {
            candidates.push(dir.join(r"Tailscale\tailscale.exe"));
        }
        candidates.push(PathBuf::from(r"C:\Program Files\Tailscale\tailscale.exe"));
        Self { candidates }
    }
}

impl Tailscale for TailscaleCli {
    fn probe(&self) -> Tailnet {
        let Some(binary) = self.candidates.iter().find(|p| p.is_file()) else {
            return Tailnet::Missing;
        };
        match run_bounded(binary, &["ip", "-4"], Duration::from_secs(3)) {
            Some(out) => parse_tailscale_ip(&out),
            None => Tailnet::Down,
        }
    }
}

/// Tailscale's CGNAT range, 100.64.0.0/10 (as `tailnet.js`).
pub fn tailnet_ipv4(text: &str) -> bool {
    text.parse::<std::net::Ipv4Addr>().is_ok_and(|ip| {
        let [a, b, ..] = ip.octets();
        a == 100 && (64..=127).contains(&b)
    })
}

/// Exactly one tailnet address, or the tailnet is not usable.
pub fn parse_tailscale_ip(output: &str) -> Tailnet {
    match output.split_whitespace().collect::<Vec<_>>().as_slice() {
        [one] if tailnet_ipv4(one) => Tailnet::Address((*one).to_string()),
        _ => Tailnet::Down,
    }
}

/// Run a read-only query with a deadline; `None` on failure or timeout.
fn run_bounded(binary: &Path, args: &[&str], timeout: Duration) -> Option<String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut child = Command::new(binary)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .ok()?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut out = String::new();
                child.stdout.take()?.read_to_string(&mut out).ok()?;
                return status.success().then_some(out);
            }
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

/// A process this host started, identified by pid and creation time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Process {
    pub pid: u32,
    pub created: u64,
}

/// One bridge launch: program, arguments, the complete environment.
#[derive(Debug, Clone)]
pub struct Launch {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub env: Vec<(String, String)>,
    pub cwd: PathBuf,
    pub log: Option<PathBuf>,
}

impl Launch {
    #[cfg(test)]
    pub fn var(&self, key: &str) -> Option<&str> {
        self.env
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_str())
    }
}

pub trait Launcher {
    /// Start without a console, outside this console's process group.
    fn spawn_detached(&self, launch: &Launch) -> io::Result<Process>;
    /// Run in the foreground; the exit code.
    fn run(&self, launch: &Launch) -> io::Result<i32>;
    /// The live process `pid`, if any.
    fn identify(&self, pid: u32) -> Option<Process>;
    fn alive(&self, process: &Process) -> bool;
    fn terminate(&self, process: &Process) -> io::Result<()>;
}

/// Real processes.
pub struct Os;

impl Os {
    fn command(launch: &Launch) -> Command {
        let mut command = Command::new(&launch.program);
        command
            .args(&launch.args)
            .current_dir(&launch.cwd)
            .env_clear()
            .envs(launch.env.iter().map(|(k, v)| (k, v)));
        command
    }
}

mod process {
    use super::Process;
    use std::io;
    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, HANDLE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        PROCESS_TERMINATE, TerminateProcess,
    };

    const STILL_ACTIVE: u32 = 259;

    struct Handle(HANDLE);
    impl Drop for Handle {
        fn drop(&mut self) {
            // SAFETY: a process handle we opened.
            unsafe { CloseHandle(self.0) };
        }
    }

    fn open(pid: u32, access: u32) -> Option<Handle> {
        // SAFETY: OpenProcess has no memory preconditions.
        let handle = unsafe { OpenProcess(access, 0, pid) };
        (!handle.is_null()).then_some(Handle(handle))
    }

    /// Creation time of a running process.
    fn live_created(handle: &Handle) -> Option<u64> {
        let mut code = 0u32;
        // SAFETY: a live process handle and valid out-pointers.
        unsafe {
            if GetExitCodeProcess(handle.0, &mut code) == 0 || code != STILL_ACTIVE {
                return None;
            }
            let zero = FILETIME {
                dwLowDateTime: 0,
                dwHighDateTime: 0,
            };
            let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
            if GetProcessTimes(handle.0, &mut created, &mut exited, &mut kernel, &mut user) == 0 {
                return None;
            }
            Some((u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
        }
    }

    pub fn identify(pid: u32) -> Option<Process> {
        let handle = open(pid, PROCESS_QUERY_LIMITED_INFORMATION)?;
        live_created(&handle).map(|created| Process { pid, created })
    }

    pub fn terminate(process: &Process) -> io::Result<()> {
        let Some(handle) = open(
            process.pid,
            PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION,
        ) else {
            return Ok(());
        };
        // Never a recycled pid: the creation time must still match.
        if live_created(&handle) != Some(process.created) {
            return Ok(());
        }
        // SAFETY: a live process handle opened with PROCESS_TERMINATE.
        if unsafe { TerminateProcess(handle.0, 1) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

impl Launcher for Os {
    fn spawn_detached(&self, launch: &Launch) -> io::Result<Process> {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let log = || -> io::Result<Stdio> {
            match &launch.log {
                Some(path) => {
                    let file = pu_core::private_fs::open_private_file(path)?;
                    let mut file = file;
                    use std::io::Seek;
                    file.seek(io::SeekFrom::End(0))?;
                    Ok(Stdio::from(file))
                }
                None => Ok(Stdio::null()),
            }
        };
        let mut command = Self::command(launch);
        command
            .stdin(Stdio::null())
            .stdout(log()?)
            .stderr(log()?)
            .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
        let child = command.spawn()?;
        let pid = child.id();
        // Dropping the Child neither waits for nor kills it.
        drop(child);
        process::identify(pid).ok_or_else(|| {
            io::Error::other(format!(
                "Point Guard exited at once; see {}",
                launch
                    .log
                    .as_deref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default()
            ))
        })
    }

    fn run(&self, launch: &Launch) -> io::Result<i32> {
        let status = Self::command(launch).status()?;
        Ok(status.code().unwrap_or(1))
    }

    fn identify(&self, pid: u32) -> Option<Process> {
        process::identify(pid)
    }

    fn alive(&self, process: &Process) -> bool {
        process::identify(process.pid).is_some_and(|p| p.created == process.created)
    }

    fn terminate(&self, process: &Process) -> io::Result<()> {
        process::terminate(process)
    }
}

/// Everything `pu phone` touches on Windows.
pub struct Host {
    pub home: PathBuf,
    pub state: PathBuf,
    pub payload_root: PathBuf,
    pub pu: PathBuf,
    pub pu_version: String,
    pub system_root: PathBuf,
    /// Host basics copied into the bridge environment (TEMP, USERNAME, ...).
    pub passthrough: Vec<(String, String)>,
    pub autostart: Box<dyn Autostart>,
    pub tailscale: Box<dyn Tailscale>,
    pub launcher: Box<dyn Launcher>,
    pub stop_timeout: Duration,
}

/// Variables the bridge and its Pi child need from a Windows session.
const PASSTHROUGH: &[&str] = &[
    "APPDATA",
    "LOCALAPPDATA",
    "TEMP",
    "TMP",
    "USERNAME",
    "USERDOMAIN",
    "COMPUTERNAME",
    "ProgramFiles",
    "ProgramFiles(x86)",
    "ProgramData",
    "PATHEXT",
    "ComSpec",
    "NUMBER_OF_PROCESSORS",
    "PROCESSOR_ARCHITECTURE",
    "OS",
    "LANG",
    "TZ",
];

/// Strip the `\\?\` prefix `canonicalize` adds (kept for UNC paths).
fn display_path(path: &Path) -> String {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC") => rest.to_string(),
        _ => text.into_owned(),
    }
}

impl Host {
    /// The real host: this user's registry, Tailscale and processes.
    pub fn real() -> Result<Self, CliError> {
        use pu_core::canon::CanonicalPath;
        let home = pu_core::paths::home_dir()
            .ok()
            .filter(|p| p.is_absolute())
            .ok_or_else(|| CliError::Other("HOME is not set to an absolute path".into()))?;
        let pu = std::env::current_exe()?
            .canonical()
            .map_err(|e| CliError::Other(format!("cannot locate pu: {e}")))?;
        let system_root = std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        Ok(Host {
            state: phone::state_dir()?,
            payload_root: phone::payload_root()?,
            home,
            pu: PathBuf::from(display_path(&pu)),
            pu_version: phone::PU_VERSION.to_string(),
            system_root,
            passthrough: PASSTHROUGH
                .iter()
                .filter_map(|k| std::env::var(k).ok().map(|v| (k.to_string(), v)))
                .collect(),
            autostart: Box::new(RunKey),
            tailscale: Box::new(TailscaleCli::standard()),
            launcher: Box::new(Os),
            stop_timeout: Duration::from_secs(30),
        })
    }

    fn payload_dir(&self) -> PathBuf {
        self.payload_root.join(&self.pu_version)
    }

    /// The payload this `pu` was released with, verified: matching manifest,
    /// every file present, none a link, none writable by another user.
    pub fn payload(&self) -> Result<PathBuf, CliError> {
        let payload = self.payload_dir();
        phone::read_manifest_for(&payload, &self.pu_version)?;
        for file in [NODE_BIN, LOCK_BIN, BRIDGE_ENTRY] {
            let path = payload.join(file);
            let meta = std::fs::symlink_metadata(&path).map_err(|_| {
                CliError::Other(format!(
                    "phone access payload is incomplete: {} missing",
                    path.display()
                ))
            })?;
            if meta.file_type().is_symlink()
                || pu_core::win_security::check_trusted_file(&path).is_err()
            {
                return Err(CliError::Other(format!(
                    "{} is writable by others or not owned by you; refusing to run it",
                    path.display()
                )));
            }
        }
        Ok(payload)
    }

    /// The Run value's data: this `pu`, quoted.
    pub fn run_command(&self) -> String {
        format!("\"{}\" phone start", display_path(&self.pu))
    }

    /// The explicit environment the bridge runs with.
    pub fn bridge_environment(&self, payload: &Path, port: u16) -> Vec<(String, String)> {
        let pu_home = self.home.join(".pu");
        let root = self.system_root.display().to_string();
        let mut env: Vec<(String, String)> = self
            .passthrough
            .iter()
            .filter(|(k, _)| PASSTHROUGH.iter().any(|p| p.eq_ignore_ascii_case(k)))
            .cloned()
            .collect();
        env.extend([
            ("HOME".into(), self.home.display().to_string()),
            ("USERPROFILE".into(), self.home.display().to_string()),
            ("SystemRoot".into(), root.clone()),
            ("windir".into(), root.clone()),
            (
                "PATH".into(),
                format!(
                    "{};{root}\\System32;{root};{root}\\System32\\Wbem;{root}\\System32\\WindowsPowerShell\\v1.0",
                    pu_home.join("bin").display()
                ),
            ),
            (
                "POINT_GUARD_STATE_DIR".into(),
                self.state.display().to_string(),
            ),
            ("POINT_GUARD_PU_PATH".into(), self.pu.display().to_string()),
            ("POINT_GUARD_PU_HOME".into(), pu_home.display().to_string()),
            (
                "POINT_GUARD_LOCK_HELPER_PATH".into(),
                payload.join(LOCK_BIN).display().to_string(),
            ),
            // Headless: the bridge finds and binds the tailnet itself.
            ("POINT_GUARD_DISCOVER_TAILNET".into(), "1".into()),
            ("PI_MOBILE_PORT".into(), port.to_string()),
        ]);
        env
    }

    pub fn launch(&self, payload: &Path, port: u16) -> Launch {
        Launch {
            program: payload.join(NODE_BIN),
            args: vec![
                payload.join(BRIDGE_ENTRY).into_os_string(),
                "--managed".into(),
            ],
            env: self.bridge_environment(payload, port),
            cwd: self.home.clone(),
            log: Some(self.payload_root.join(LOG)),
        }
    }

    fn record(&self) -> Value {
        std::fs::read(self.payload_root.join(RECORD))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .filter(|v| v["schemaVersion"] == json!(1))
            .unwrap_or_else(|| json!({"schemaVersion": 1}))
    }

    fn save_record(&self, port: u16, process: Option<Process>) -> Result<(), CliError> {
        let mut value = json!({"schemaVersion": 1, "port": port});
        if let Some(p) = process {
            value["pid"] = json!(p.pid);
            value["created"] = json!(p.created);
        }
        pu_core::private_fs::create_private_dir(&self.payload_root)?;
        pu_core::private_fs::write_private_file(
            &self.payload_root.join(RECORD),
            &serde_json::to_vec_pretty(&value)?,
        )?;
        Ok(())
    }

    /// The installed port (`install --port`), else the default.
    pub fn port(&self) -> u16 {
        self.record()["port"]
            .as_u64()
            .and_then(|p| u16::try_from(p).ok())
            .unwrap_or(phone::DEFAULT_PORT)
    }

    /// The bridge this host started, while it still runs.
    pub fn running(&self) -> Option<Process> {
        let record = self.record();
        let process = Process {
            pid: u32::try_from(record["pid"].as_u64()?).ok()?,
            created: record["created"].as_u64()?,
        };
        self.launcher.alive(&process).then_some(process)
    }

    fn wait_exit(&self, process: &Process) -> bool {
        let deadline = Instant::now() + self.stop_timeout;
        while Instant::now() < deadline {
            if !self.launcher.alive(process) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        !self.launcher.alive(process)
    }

    fn tailnet_note(&self) -> Option<String> {
        match self.tailscale.probe() {
            Tailnet::Missing => Some(
                "Tailscale is not installed; phones reach this PC only over your tailnet. Install it from https://tailscale.com/download/windows".into(),
            ),
            Tailnet::Down => Some(
                "Tailscale is not connected; phone access starts once `tailscale up` succeeds".into(),
            ),
            Tailnet::Address(_) => None,
        }
    }

    pub fn install(&self, now: bool, port: u16) -> Result<Vec<String>, CliError> {
        let payload = self.payload()?;
        let running = self.running();
        self.save_record(port, running)?;
        self.autostart.write(&self.run_command())?;
        let mut lines = vec![format!(
            "wrote {} (runs {} at logon)",
            self.autostart.display(),
            payload.display()
        )];
        if now {
            lines.extend(self.start()?);
            lines.push("pair a phone with `pu phone pair`".into());
        } else {
            lines.push("start it with `pu phone start`".into());
        }
        if self.autostart.disabled() {
            lines.push(
                "the entry is disabled in Settings > Apps > Startup; enable it there for Point Guard to start at logon".into(),
            );
        }
        lines.extend(self.tailnet_note());
        Ok(lines)
    }

    /// `start` runs only the Run entry this `pu` writes, so an older `pu`
    /// left registered after an upgrade never breaks the version lockstep.
    pub fn start(&self) -> Result<Vec<String>, CliError> {
        let payload = self.payload()?;
        let expected = self.run_command();
        match self.autostart.read()? {
            None => {
                return Err(CliError::Other(format!(
                    "{RUN_VALUE} is not installed; install it with `pu phone install --now`"
                )));
            }
            Some(installed) if installed != expected => {
                return Err(CliError::Other(format!(
                    "{RUN_VALUE} runs `{installed}`, but this pu is {}; move it with `pu phone restart` (or `pu phone install --now`)",
                    self.pu.display()
                )));
            }
            Some(_) => {}
        }
        if let Some(process) = self.running() {
            return Ok(vec![format!(
                "Point Guard is already running (pid {})",
                process.pid
            )]);
        }
        let port = self.port();
        let process = self.launcher.spawn_detached(&self.launch(&payload, port))?;
        self.save_record(port, Some(process))?;
        Ok(vec![format!(
            "started Point Guard (pid {}); check `pu phone status`",
            process.pid
        )])
    }

    /// Ask the bridge to drain and exit while idle; it refuses during work.
    /// Fails closed when it runs but its admin channel can't be reached.
    pub fn stop_idle(&self) -> Result<bool, CliError> {
        let running = self.running();
        let unavailable = |e: CliError| {
            CliError::Other(format!(
                "Point Guard is running but its admin channel is unavailable ({e}), so it can't be stopped only when idle; retry shortly, or pass --force to stop it anyway (unfinished replies are lost)"
            ))
        };
        let admin = match Admin::connect(&self.state) {
            Ok(admin) => admin,
            Err(_) if running.is_none() => return Ok(false),
            Err(e) => return Err(unavailable(e)),
        };
        // The bridge names its own process; prefer that over the record.
        let status = match admin.call(json!({"operation": "status"})) {
            Ok(status) => status,
            Err(_) if running.is_none() => return Ok(false),
            Err(e) => return Err(unavailable(e)),
        };
        let target = status["pid"]
            .as_u64()
            .and_then(|pid| u32::try_from(pid).ok())
            .and_then(|pid| self.launcher.identify(pid))
            .or(running);
        admin.call(json!({"operation": "runtime.stop"}))?;
        match target {
            Some(process) if !self.wait_exit(&process) => Err(CliError::Other(format!(
                "Point Guard did not stop within {}s",
                self.stop_timeout.as_secs()
            ))),
            _ => Ok(true),
        }
    }

    /// End a bridge this host started that is still running.
    fn terminate_running(&self) -> Result<(), CliError> {
        if let Some(process) = self.running() {
            self.launcher.terminate(&process)?;
            if !self.wait_exit(&process) {
                return Err(CliError::Other(format!(
                    "Point Guard (pid {}) did not exit",
                    process.pid
                )));
            }
        }
        Ok(())
    }

    pub fn stop(&self, force: bool) -> Result<Vec<String>, CliError> {
        if !force {
            self.stop_idle()?;
        }
        self.terminate_running()?;
        self.save_record(self.port(), None)?;
        Ok(vec!["stopped".into()])
    }

    pub fn restart(&self, force: bool) -> Result<Vec<String>, CliError> {
        let payload = self.payload()?;
        if !force {
            self.stop_idle()?;
        }
        self.terminate_running()?;
        let expected = self.run_command();
        if self.autostart.read()?.as_deref() != Some(expected.as_str()) {
            self.autostart.write(&expected)?;
        }
        let mut lines = self.start()?;
        self.prune_payloads(&payload);
        lines.push(format!("restarted on {}", payload.display()));
        Ok(lines)
    }

    /// Remove payload versions nothing runs any more: only plain `X.Y.Z`
    /// directories older than `keep`, never newer ones or staging folders.
    pub fn prune_payloads(&self, keep: &Path) {
        let Ok(entries) = std::fs::read_dir(&self.payload_root) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if entry.file_type().is_ok_and(|t| t.is_dir())
                && path != keep
                && phone::prunable(&entry.file_name().to_string_lossy(), keep)
            {
                let _ = pu_core::fs_retry::remove_dir_all(&path);
            }
        }
    }

    pub fn uninstall(&self) -> Result<Vec<String>, CliError> {
        let removed = self.autostart.delete()?;
        let running = self.running();
        if !removed && running.is_none() {
            return Ok(vec!["phone service is not installed".into()]);
        }
        let mut lines = Vec::new();
        if running.is_some() {
            match self.stop_idle() {
                Ok(_) => self.save_record(self.port(), None)?,
                Err(e) => lines.push(format!(
                    "Point Guard is still running ({e}); stop it with `pu phone stop --force`"
                )),
            }
        }
        lines.push(format!(
            "removed {}; paired devices and Point Guard state are kept in {}",
            self.autostart.display(),
            self.state.display()
        ));
        Ok(lines)
    }

    fn service_state(&self) -> String {
        match self.autostart.read() {
            Ok(None) => "not registered".into(),
            Err(e) => format!("unknown ({e})"),
            Ok(Some(_)) if self.autostart.disabled() => {
                "registered, disabled in Startup apps".into()
            }
            Ok(Some(_)) => "registered (starts at logon)".into(),
        }
    }

    /// Status as JSON: autostart, process, Tailscale and the bridge's own view.
    pub fn status_value(&self) -> Value {
        let running = self.running();
        let tailscale = match self.tailscale.probe() {
            Tailnet::Missing => json!({"state": "missing"}),
            Tailnet::Down => json!({"state": "down"}),
            Tailnet::Address(address) => json!({"state": "connected", "address": address}),
        };
        let mut value = json!({
            "service": self.service_state(),
            "running": running.is_some(),
            "tailscale": tailscale,
        });
        if let Some(p) = running {
            value["pid"] = json!(p.pid);
        }
        match Admin::connect(&self.state).and_then(|a| a.call(json!({"operation": "status"}))) {
            Ok(s) => value["pointGuard"] = s,
            Err(e) => value["error"] = json!(e.to_string()),
        }
        value
    }

    pub fn status_lines(&self, value: &Value) -> Vec<String> {
        let mut lines = vec![format!(
            "service:   {}",
            value["service"].as_str().unwrap_or("?")
        )];
        lines.push(match value["pid"].as_u64() {
            Some(pid) => format!("process:   running (pid {pid})"),
            None => "process:   not running in the background (`pu phone start`)".into(),
        });
        lines.push(match value["tailscale"]["state"].as_str() {
            Some("connected") => format!(
                "tailscale: {}",
                value["tailscale"]["address"].as_str().unwrap_or("?")
            ),
            Some("missing") => {
                "tailscale: not installed; phones can't reach this PC until Tailscale is installed and connected".into()
            }
            _ => "tailscale: not connected; run `tailscale up`".into(),
        });
        match &value["pointGuard"] {
            Value::Null => lines.push(format!(
                "bridge:    {}",
                value["error"].as_str().unwrap_or("unavailable")
            )),
            s => lines.extend(crate::commands::phone::bridge_status_lines(s)),
        }
        lines
    }

    pub fn run_foreground(&self, port: u16) -> Result<(), CliError> {
        let payload = self.payload()?;
        let mut launch = self.launch(&payload, port);
        launch.log = None;
        let code = self
            .launcher
            .run(&launch)
            .map_err(|e| CliError::Other(format!("cannot start Point Guard: {e}")))?;
        if code == 0 {
            Ok(())
        } else {
            Err(CliError::Other(format!("Point Guard exited ({code})")))
        }
    }
}

/// Test doubles and a fake bridge admin pipe, shared with `commands::phone`.
#[cfg(test)]
pub(crate) mod fakes {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    pub struct Registry {
        pub value: Arc<Mutex<Option<String>>>,
        pub disabled: bool,
    }

    impl Autostart for Registry {
        fn read(&self) -> io::Result<Option<String>> {
            Ok(self.value.lock().unwrap().clone())
        }
        fn write(&self, command: &str) -> io::Result<()> {
            *self.value.lock().unwrap() = Some(command.into());
            Ok(())
        }
        fn delete(&self) -> io::Result<bool> {
            Ok(self.value.lock().unwrap().take().is_some())
        }
        fn disabled(&self) -> bool {
            self.disabled
        }
        fn display(&self) -> String {
            format!("fake Run\\{RUN_VALUE}")
        }
    }

    pub struct FixedTailnet(pub Tailnet);

    impl Tailscale for FixedTailnet {
        fn probe(&self) -> Tailnet {
            self.0.clone()
        }
    }

    /// Fake processes: spawns are recorded and stay alive until terminated
    /// or until the fake bridge handles `runtime.stop`.
    #[derive(Clone, Default)]
    pub struct Processes {
        pub launches: Arc<Mutex<Vec<Launch>>>,
        pub live: Arc<Mutex<Vec<Process>>>,
        pub terminated: Arc<Mutex<Vec<u32>>>,
    }

    impl Processes {
        pub fn exit_all(&self) {
            self.live.lock().unwrap().clear();
        }
    }

    impl Launcher for Processes {
        fn spawn_detached(&self, launch: &Launch) -> io::Result<Process> {
            let mut launches = self.launches.lock().unwrap();
            launches.push(launch.clone());
            let process = Process {
                pid: 40_000 + launches.len() as u32,
                created: 7,
            };
            self.live.lock().unwrap().push(process);
            Ok(process)
        }
        fn run(&self, launch: &Launch) -> io::Result<i32> {
            self.launches.lock().unwrap().push(launch.clone());
            Ok(0)
        }
        fn identify(&self, pid: u32) -> Option<Process> {
            self.live
                .lock()
                .unwrap()
                .iter()
                .find(|p| p.pid == pid)
                .copied()
        }
        fn alive(&self, process: &Process) -> bool {
            self.live.lock().unwrap().contains(process)
        }
        fn terminate(&self, process: &Process) -> io::Result<()> {
            self.terminated.lock().unwrap().push(process.pid);
            self.live.lock().unwrap().retain(|p| p != process);
            Ok(())
        }
    }

    pub struct Fixture {
        pub _root: tempfile::TempDir,
        pub host: Host,
        pub registry: Registry,
        pub processes: Processes,
    }

    /// A temp home with a complete payload for `version`.
    pub fn fixture(tailnet: Tailnet) -> Fixture {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let payload_root = home.join(r".pu\pointguard");
        let version = "1.2.3";
        let payload = payload_root.join(version);
        for file in [NODE_BIN, LOCK_BIN, BRIDGE_ENTRY] {
            let path = payload.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, b"fixture").unwrap();
        }
        std::fs::write(
            payload.join("runtime-manifest.json"),
            json!({"schemaVersion": 1, "layout": "windows", "puVersion": version}).to_string(),
        )
        .unwrap();
        let registry = Registry::default();
        let processes = Processes::default();
        let host = Host {
            state: phone::windows_state_dir_from(
                Some(&home.join(r"AppData\Local").display().to_string()),
                &home,
            ),
            payload_root,
            pu: home.join(r".pu\bin\pu.exe"),
            pu_version: version.into(),
            system_root: PathBuf::from(r"C:\Windows"),
            passthrough: vec![
                ("TEMP".into(), r"C:\t".into()),
                ("NODE_OPTIONS".into(), "--require evil".into()),
            ],
            home,
            autostart: Box::new(registry.clone()),
            tailscale: Box::new(FixedTailnet(tailnet)),
            launcher: Box::new(processes.clone()),
            stop_timeout: Duration::from_secs(2),
        };
        Fixture {
            _root: root,
            host,
            registry,
            processes,
        }
    }

    type Handler = Arc<dyn Fn(&Value) -> Result<Value, String> + Send + Sync>;

    /// A fake bridge: a private state directory with an owner token and an
    /// owner pipe that answers admin requests with `handler`.
    pub struct Bridge {
        pub requests: Arc<Mutex<Vec<Value>>>,
        pub tokens: Arc<Mutex<Vec<String>>>,
    }

    pub const TOKEN: &str = "fixture-owner-token-0123456789abcdef0123456789";

    pub fn bridge(
        state: &Path,
        handler: impl Fn(&Value) -> Result<Value, String> + Send + Sync + 'static,
    ) -> Bridge {
        pu_core::private_fs::create_private_dir(state).unwrap();
        pu_core::private_fs::write_private_file(&state.join("admin-token"), TOKEN.as_bytes())
            .unwrap();
        let name = phone::admin_pipe(state).unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let tokens = Arc::new(Mutex::new(Vec::new()));
        let handler: Handler = Arc::new(handler);
        let (ready, wait) = std::sync::mpsc::channel();
        let (seen, auth) = (requests.clone(), tokens.clone());
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(serve(name, handler, seen, auth, ready));
        });
        wait.recv().unwrap();
        Bridge { requests, tokens }
    }

    async fn serve(
        name: String,
        handler: Handler,
        seen: Arc<Mutex<Vec<Value>>>,
        auth: Arc<Mutex<Vec<String>>>,
        ready: std::sync::mpsc::Sender<()>,
    ) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::windows::named_pipe::ServerOptions;
        let mut server = ServerOptions::new()
            .first_pipe_instance(true)
            .create(&name)
            .unwrap();
        let _ = ready.send(());
        loop {
            if server.connect().await.is_err() {
                return;
            }
            let mut pipe = server;
            server = ServerOptions::new().create(&name).unwrap();
            let mut raw = Vec::new();
            let mut buf = [0u8; 4096];
            let body = loop {
                let n = pipe.read(&mut buf).await.unwrap_or(0);
                if n == 0 {
                    break None;
                }
                raw.extend_from_slice(&buf[..n]);
                let Some(split) = raw.windows(4).position(|w| w == b"\r\n\r\n") else {
                    continue;
                };
                let head = String::from_utf8_lossy(&raw[..split]).to_string();
                let length: usize = head
                    .lines()
                    .find_map(|l| l.strip_prefix("Content-Length: "))
                    .and_then(|v| v.trim().parse().ok())
                    .unwrap_or(0);
                if raw.len() >= split + 4 + length {
                    if let Some(token) = head
                        .lines()
                        .find_map(|l| l.strip_prefix("Authorization: Bearer "))
                    {
                        auth.lock().unwrap().push(token.to_string());
                    }
                    break Some(raw[split + 4..split + 4 + length].to_vec());
                }
            };
            let Some(body) = body else { continue };
            let request: Value = serde_json::from_slice(&body).unwrap();
            seen.lock().unwrap().push(request.clone());
            let (status, reply) = match handler(&request) {
                Ok(result) => ("200 OK", json!({"ok": true, "result": result})),
                Err(message) => (
                    "409 Conflict",
                    json!({"ok": false, "error": {"code": "busy", "message": message}}),
                ),
            };
            let reply = reply.to_string();
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{reply}",
                reply.len()
            );
            let _ = pipe.write_all(response.as_bytes()).await;
            let _ = pipe.flush().await;
            // Closing (not DisconnectNamedPipe) leaves the reply readable.
            drop(pipe);
        }
    }

    pub fn operations(bridge: &Bridge) -> Vec<String> {
        bridge
            .requests
            .lock()
            .unwrap()
            .iter()
            .map(|r| r["operation"].as_str().unwrap_or("?").to_string())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::fakes::*;
    use super::*;

    fn installed(f: &Fixture) -> Option<String> {
        f.registry.value.lock().unwrap().clone()
    }

    #[test]
    fn given_tailscale_output_should_accept_only_one_tailnet_ipv4() {
        assert_eq!(
            parse_tailscale_ip("100.65.202.51\r\n"),
            Tailnet::Address("100.65.202.51".into())
        );
        for bad in [
            "",
            "192.168.4.32",
            "100.128.0.1",
            "100.64.0.1 100.64.0.2",
            "x",
        ] {
            assert_eq!(parse_tailscale_ip(bad), Tailnet::Down, "{bad:?}");
        }
        assert!(tailnet_ipv4("100.127.255.255") && !tailnet_ipv4("100.63.0.1"));
        let none = TailscaleCli {
            candidates: vec![PathBuf::from(r"C:\nonexistent\tailscale.exe")],
        };
        assert_eq!(none.probe(), Tailnet::Missing);
    }

    #[test]
    fn given_install_should_register_logon_entry_and_report_missing_tailscale() {
        let f = fixture(Tailnet::Missing);
        let lines = f.host.install(false, 9000).unwrap();
        let expected = format!("\"{}\" phone start", f.host.pu.display());
        assert_eq!(installed(&f), Some(expected));
        assert_eq!(f.host.port(), 9000);
        assert!(
            lines.iter().any(|l| l.contains("pu phone start")),
            "{lines:?}"
        );
        assert!(
            lines
                .iter()
                .any(|l| l.contains("Tailscale is not installed")),
            "{lines:?}"
        );
        assert!(f.processes.launches.lock().unwrap().is_empty());
    }

    #[test]
    fn given_install_now_should_launch_the_payload_bridge_with_an_explicit_environment() {
        let f = fixture(Tailnet::Address("100.64.1.2".into()));
        let lines = f.host.install(true, 8787).unwrap();
        assert!(!lines.iter().any(|l| l.contains("Tailscale")), "{lines:?}");
        let launches = f.processes.launches.lock().unwrap();
        let [launch] = launches.as_slice() else {
            panic!("{launches:?}")
        };
        let payload = f.host.payload_root.join("1.2.3");
        assert_eq!(launch.program, payload.join(NODE_BIN));
        assert_eq!(
            launch.args,
            vec![
                payload.join(BRIDGE_ENTRY).into_os_string(),
                OsString::from("--managed")
            ]
        );
        assert_eq!(launch.var("POINT_GUARD_DISCOVER_TAILNET"), Some("1"));
        assert_eq!(launch.var("PI_MOBILE_PORT"), Some("8787"));
        assert_eq!(
            launch.var("POINT_GUARD_LOCK_HELPER_PATH"),
            Some(payload.join(LOCK_BIN).to_str().unwrap())
        );
        assert_eq!(
            launch.var("POINT_GUARD_STATE_DIR"),
            Some(f.host.state.to_str().unwrap())
        );
        assert!(
            f.host
                .state
                .ends_with(r"AppData\Local\PurePoint\point-guard")
        );
        assert_eq!(launch.var("TEMP"), Some(r"C:\t"));
        assert_eq!(launch.var("NODE_OPTIONS"), None, "never passed through");
        assert_eq!(launch.var("PI_MOBILE_HOST"), None, "the bridge discovers");
        assert_eq!(launch.var("POINT_GUARD_SUPERVISOR"), None);
        drop(launches);
        assert!(f.host.running().is_some());
        // A second start is a no-op while the recorded bridge runs.
        let again = f.host.start().unwrap();
        assert!(again[0].contains("already running"), "{again:?}");
        assert_eq!(f.processes.launches.lock().unwrap().len(), 1);
    }

    #[test]
    fn given_incomplete_or_foreign_payload_should_refuse_to_install() {
        let f = fixture(Tailnet::Missing);
        let payload = f.host.payload_root.join("1.2.3");
        std::fs::remove_file(payload.join(LOCK_BIN)).unwrap();
        let err = f.host.install(true, 8787).unwrap_err().to_string();
        assert!(err.contains("incomplete"), "{err}");
        std::fs::write(payload.join(LOCK_BIN), b"x").unwrap();
        std::fs::write(
            payload.join("runtime-manifest.json"),
            json!({"schemaVersion": 1, "layout": "linux", "puVersion": "1.2.3"}).to_string(),
        )
        .unwrap();
        assert!(f.host.install(true, 8787).is_err());
        assert_eq!(installed(&f), None);
        let g = fixture(Tailnet::Missing);
        let shared = g.host.payload_root.join("1.2.3").join(BRIDGE_ENTRY);
        pu_core::win_security::grant_for_test(&shared, "WD").unwrap();
        let err = g.host.install(false, 8787).unwrap_err().to_string();
        assert!(err.contains("writable by others"), "{err}");
    }

    #[test]
    fn given_missing_or_foreign_run_entry_should_refuse_to_start() {
        let f = fixture(Tailnet::Missing);
        let err = f.host.start().unwrap_err().to_string();
        assert!(err.contains("pu phone install --now"), "{err}");
        f.registry.write(r#""C:\old\pu.exe" phone start"#).unwrap();
        let err = f.host.start().unwrap_err().to_string();
        assert!(err.contains("pu phone restart"), "{err}");
        assert!(f.processes.launches.lock().unwrap().is_empty());
    }

    #[test]
    fn given_running_bridge_should_restart_only_once_idle_and_move_to_this_pu() {
        let f = fixture(Tailnet::Missing);
        f.host.install(true, 8787).unwrap();
        let first = f.host.running().unwrap();
        let processes = f.processes.clone();
        let bridge = bridge(&f.host.state, move |r| match r["operation"].as_str() {
            Some("status") => Ok(json!({"pid": first.pid, "phase": "ready"})),
            Some("runtime.stop") => {
                processes.exit_all();
                Ok(json!({}))
            }
            _ => Err("unexpected".into()),
        });
        // An older pu left registered, and an older payload left behind.
        f.registry.write(r#""C:\old\pu.exe" phone start"#).unwrap();
        let old = f.host.payload_root.join("1.2.2");
        std::fs::create_dir_all(&old).unwrap();
        let newer = f.host.payload_root.join("1.3.0");
        std::fs::create_dir_all(&newer).unwrap();
        let lines = f.host.restart(false).unwrap();
        assert_eq!(operations(&bridge), ["status", "runtime.stop"]);
        assert_eq!(bridge.tokens.lock().unwrap().as_slice(), [TOKEN, TOKEN]);
        assert!(f.processes.terminated.lock().unwrap().is_empty());
        assert_eq!(installed(&f), Some(f.host.run_command()));
        assert_eq!(f.processes.launches.lock().unwrap().len(), 2);
        let second = f.host.running().unwrap();
        assert_ne!(second, first);
        assert!(!old.exists() && newer.exists());
        assert!(lines.last().unwrap().contains("restarted on"), "{lines:?}");
    }

    #[test]
    fn given_busy_bridge_should_not_restart_unless_forced() {
        let f = fixture(Tailnet::Missing);
        f.host.install(true, 8787).unwrap();
        let first = f.host.running().unwrap();
        let bridge = bridge(&f.host.state, move |r| match r["operation"].as_str() {
            Some("status") => Ok(json!({"pid": first.pid})),
            _ => Err("Wait for Point Guard and its queued work to finish.".into()),
        });
        let err = f.host.restart(false).unwrap_err().to_string();
        assert!(err.contains("Wait for Point Guard"), "{err}");
        assert_eq!(f.host.running(), Some(first));
        assert_eq!(f.processes.launches.lock().unwrap().len(), 1);
        f.host.restart(true).unwrap();
        assert_eq!(
            f.processes.terminated.lock().unwrap().as_slice(),
            [first.pid]
        );
        assert_eq!(f.processes.launches.lock().unwrap().len(), 2);
        assert_eq!(operations(&bridge), ["status", "runtime.stop"]);
    }

    #[test]
    fn given_unreachable_admin_should_fail_closed_unless_forced() {
        let f = fixture(Tailnet::Missing);
        f.host.install(true, 8787).unwrap();
        let err = f.host.stop(false).unwrap_err().to_string();
        assert!(err.contains("--force"), "{err}");
        assert!(f.host.running().is_some());
        assert_eq!(f.host.stop(true).unwrap(), ["stopped"]);
        assert!(f.host.running().is_none());
        // Nothing running and no admin: a plain stop is fine.
        assert_eq!(f.host.stop(false).unwrap(), ["stopped"]);
    }

    #[test]
    fn given_status_without_tailscale_should_report_it_and_the_missing_bridge() {
        let f = fixture(Tailnet::Missing);
        let value = f.host.status_value();
        assert_eq!(value["service"], "not registered");
        assert_eq!(value["running"], false);
        assert_eq!(value["tailscale"]["state"], "missing");
        assert!(value["error"].as_str().unwrap().contains("not running"));
        let lines = f.host.status_lines(&value).join("\n");
        assert!(lines.contains("tailscale: not installed"), "{lines}");
        assert!(
            lines.contains("process:   not running in the background"),
            "{lines}"
        );
    }

    #[test]
    fn given_status_with_a_running_bridge_should_show_its_view() {
        let f = fixture(Tailnet::Address("100.64.1.2".into()));
        f.host.install(true, 8787).unwrap();
        let _bridge = bridge(&f.host.state, |r| match r["operation"].as_str() {
            Some("status") => Ok(json!({
                "phase": "ready", "hostLabel": "pc", "hostKind": "windows",
                "remote": {"state": "unavailable", "message": "Tailscale is not connected.", "recovery": "Run `tailscale up`."},
                "cwd": r"C:\work",
            })),
            _ => Err("unexpected".into()),
        });
        let value = f.host.status_value();
        assert_eq!(value["tailscale"]["address"], "100.64.1.2");
        assert_eq!(value["pointGuard"]["phase"], "ready");
        let lines = f.host.status_lines(&value).join("\n");
        assert!(
            lines.contains("service:   registered (starts at logon)"),
            "{lines}"
        );
        assert!(lines.contains("tailscale: 100.64.1.2"), "{lines}");
        assert!(lines.contains("phones:    not reachable"), "{lines}");
        assert!(lines.contains("Tailscale is not connected."), "{lines}");
        assert!(lines.contains(r"folder:    C:\work"), "{lines}");
    }

    #[test]
    fn given_uninstall_should_remove_the_entry_and_keep_state() {
        let f = fixture(Tailnet::Missing);
        assert_eq!(
            f.host.uninstall().unwrap(),
            ["phone service is not installed"]
        );
        f.host.install(false, 8787).unwrap();
        let lines = f.host.uninstall().unwrap();
        assert_eq!(installed(&f), None);
        assert!(lines[0].contains("are kept in"), "{lines:?}");
    }

    #[test]
    fn given_run_should_run_the_bridge_in_the_foreground_without_a_log() {
        let f = fixture(Tailnet::Missing);
        f.host.run_foreground(9001).unwrap();
        let launches = f.processes.launches.lock().unwrap();
        assert_eq!(launches.len(), 1);
        assert_eq!(launches[0].var("PI_MOBILE_PORT"), Some("9001"));
        assert!(launches[0].log.is_none());
        assert!(f.processes.live.lock().unwrap().is_empty());
        assert_eq!(installed(&f), None, "run never registers autostart");
    }

    #[test]
    fn given_real_processes_should_track_by_creation_time_and_terminate() {
        let root = tempfile::tempdir().unwrap();
        let system = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        let launch = Launch {
            program: PathBuf::from(&system).join(r"System32\PING.EXE"),
            args: vec!["-n".into(), "60".into(), "127.0.0.1".into()],
            env: vec![("SystemRoot".into(), system)],
            cwd: root.path().to_path_buf(),
            log: Some(root.path().join("host.log")),
        };
        let process = Os.spawn_detached(&launch).unwrap();
        assert!(Os.alive(&process));
        assert_eq!(Os.identify(process.pid), Some(process));
        let recycled = Process {
            created: process.created + 1,
            ..process
        };
        assert!(
            !Os.alive(&recycled),
            "another creation time is another process"
        );
        Os.terminate(&recycled).unwrap();
        assert!(Os.alive(&process), "never terminates a recycled pid");
        Os.terminate(&process).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while Os.alive(&process) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(!Os.alive(&process));
        assert!(root.path().join("host.log").exists());
    }

    #[test]
    fn given_shared_state_or_token_should_refuse_the_admin_channel() {
        let f = fixture(Tailnet::Missing);
        let _bridge = bridge(&f.host.state, |_| Ok(json!({})));
        assert!(Admin::connect(&f.host.state).is_ok());
        pu_core::win_security::grant_for_test(&f.host.state.join("admin-token"), "WD").unwrap();
        let err = Admin::connect(&f.host.state).err().unwrap().to_string();
        assert!(err.contains("not a private regular file"), "{err}");
        pu_core::win_security::set_private_dacl(&f.host.state.join("admin-token"), false).unwrap();
        pu_core::win_security::grant_for_test(&f.host.state, "BU").unwrap();
        let err = Admin::connect(&f.host.state).err().unwrap().to_string();
        assert!(err.contains("not private"), "{err}");
    }
}

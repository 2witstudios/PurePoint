//! Port of `Services/DaemonLifecycle.swift` (REQ-WINAPP-063): find `pu-engine.exe`, reuse a
//! healthy daemon, otherwise start one tied to the app, and restart a daemon older than its
//! binary.
//!
//! - The binary: `PU_ENGINE_PATH` alone when set, else beside the app exe (the Tauri
//!   sidecar), the resource dir, then `%USERPROFILE%\.pu\bin`. Every candidate must pass
//!   `pu_core::win_security::check_trusted_binary`; the order and check are `pu`'s
//!   ([`pu_core::engine_binary`]).
//! - Reuse: a daemon answering health (or `BUSY`, which proves it alive) within three tries
//!   is kept, unless the binary is newer than its PID file (the Swift `shouldRestart`). A
//!   daemon of another protocol version is reported as [`ConnectionState::Incompatible`]
//!   with the banner text, never restarted on that ground alone: that would end its agents.
//! - Start: `pu-engine.exe --managed --parent-pid <app pid> --socket <socket>`, detached,
//!   with no window, stderr appended to `daemon.log`, then health polled with backoff.
//!
//! Operations are serialized, so N callers at launch start at most one daemon.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pu_core::protocol::{PROTOCOL_VERSION, Request, Response};
use serde::Serialize;

use super::client::{DaemonClient, DaemonError};

/// Longest a single health probe may take.
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);

/// The connection to the local daemon, as the UI shows it (`daemon-state` event).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ConnectionState {
    /// Not yet checked.
    Idle,
    /// Probing, or starting a daemon.
    Starting,
    /// A compatible daemon answers. `pid` is unknown when it answered `BUSY`.
    Connected {
        pid: Option<u32>,
        /// This app started it, so it ends with the app (`--parent-pid`).
        launched: bool,
    },
    /// The daemon speaks another protocol version.
    Incompatible {
        daemon_version: u32,
        app_version: u32,
        message: String,
    },
    /// No daemon could be reached or started. `message` is the banner text.
    Failed { kind: String, message: String },
}

impl ConnectionState {
    fn failed(error: &DaemonError) -> Self {
        match error {
            DaemonError::Incompatible { daemon, app } => Self::Incompatible {
                daemon_version: *daemon,
                app_version: *app,
                message: error.to_string(),
            },
            _ => Self::Failed {
                kind: error.kind().into(),
                message: error.to_string(),
            },
        }
    }
}

/// Where to look for `pu-engine.exe`.
#[derive(Debug, Clone, Default)]
pub struct BinaryLocator {
    /// Searched in order: the app exe's directory, then the resource directory.
    pub dirs: Vec<PathBuf>,
    /// `%USERPROFILE%`, for `.pu\bin`.
    pub home: Option<PathBuf>,
    /// `PU_ENGINE_PATH`: the only candidate when set.
    pub explicit: Option<OsString>,
}

impl BinaryLocator {
    /// The app's own locations plus the user's install and `PU_ENGINE_PATH`.
    pub fn for_app(resource_dir: Option<PathBuf>) -> Self {
        let exe_dir = std::env::current_exe()
            .and_then(pu_core::canon::canonicalize)
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf));
        Self {
            dirs: exe_dir.into_iter().chain(resource_dir).collect(),
            home: pu_core::paths::home_dir().ok(),
            explicit: std::env::var_os(pu_core::engine_binary::ENGINE_PATH_ENV)
                .filter(|v| !v.is_empty()),
        }
    }

    /// The first trusted candidate, or why each one was refused.
    pub fn find(&self) -> Result<PathBuf, String> {
        use pu_core::engine_binary;
        let candidates = match &self.explicit {
            Some(explicit) => match which::which(explicit) {
                Ok(path) => vec![path],
                Err(e) => {
                    return Err(format!(
                        "{}={}: {e}",
                        engine_binary::ENGINE_PATH_ENV,
                        explicit.to_string_lossy()
                    ));
                }
            },
            None => engine_binary::candidates(
                self.dirs.iter().map(PathBuf::as_path),
                self.home.as_deref(),
            ),
        };
        engine_binary::first_trusted(candidates)
    }
}

/// What one health probe learned.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Probe {
    Healthy {
        pid: u32,
        protocol_version: u32,
    },
    /// Alive, but turned the connection away at its connection limit.
    Busy,
    Unreachable,
}

type Listener = Box<dyn Fn(&ConnectionState) + Send + Sync>;

/// Starts and tracks the local daemon for the app.
pub struct DaemonLifecycle {
    socket: PathBuf,
    client: DaemonClient,
    locator: BinaryLocator,
    /// The process the daemon watches (`--parent-pid`): the app.
    parent_pid: u32,
    /// Extra environment for the daemon (tests point `HOME` at a temp dir).
    spawn_env: Vec<(OsString, OsString)>,
    /// PID of the daemon this app launched, while it is the one answering.
    launched_pid: Mutex<Option<u32>>,
    operation: tokio::sync::Mutex<()>,
    state: Mutex<ConnectionState>,
    listener: Mutex<Option<Listener>>,
}

impl DaemonLifecycle {
    pub fn new(socket: &Path, locator: BinaryLocator) -> std::io::Result<Self> {
        Ok(Self {
            socket: socket.to_path_buf(),
            client: DaemonClient::local(socket)?,
            locator,
            parent_pid: std::process::id(),
            spawn_env: Vec::new(),
            launched_pid: Mutex::new(None),
            operation: tokio::sync::Mutex::new(()),
            state: Mutex::new(ConnectionState::Idle),
            listener: Mutex::new(None),
        })
    }

    /// Set environment variables for a daemon this lifecycle starts.
    pub fn with_spawn_env(mut self, env: Vec<(OsString, OsString)>) -> Self {
        self.spawn_env = env;
        self
    }

    /// Called with every state change (the app emits `daemon-state`).
    pub fn on_change(&self, listener: impl Fn(&ConnectionState) + Send + Sync + 'static) {
        *lock(&self.listener) = Some(Box::new(listener));
    }

    pub fn client(&self) -> &DaemonClient {
        &self.client
    }

    pub fn state(&self) -> ConnectionState {
        lock(&self.state).clone()
    }

    /// The PID of the daemon this app started, if that daemon is the one running.
    pub fn launched_pid(&self) -> Option<u32> {
        *lock(&self.launched_pid)
    }

    fn set_state(&self, state: ConnectionState) {
        let changed = {
            let mut current = lock(&self.state);
            let changed = *current != state;
            *current = state.clone();
            changed
        };
        if changed && let Some(listener) = lock(&self.listener).as_ref() {
            listener(&state);
        }
    }

    /// Record a request failure that says the daemon is gone.
    pub fn note_failure(&self, error: &DaemonError) {
        if matches!(error, DaemonError::NotRunning(_)) {
            *lock(&self.launched_pid) = None;
            self.set_state(ConnectionState::failed(error));
        }
    }

    /// Ensure a compatible daemon is running, starting one if needed.
    pub async fn ensure_daemon(&self) -> Result<(), DaemonError> {
        let _serial = self.operation.lock().await;
        if matches!(self.state(), ConnectionState::Connected { .. })
            && self.probe().await != Probe::Unreachable
        {
            return Ok(());
        }
        self.set_state(ConnectionState::Starting);
        let result = self.ensure_now().await;
        if let Err(error) = &result {
            self.set_state(ConnectionState::failed(error));
        }
        result
    }

    /// Stop the running daemon and start a fresh one.
    pub async fn restart_daemon(&self) -> Result<(), DaemonError> {
        let _serial = self.operation.lock().await;
        self.set_state(ConnectionState::Starting);
        self.stop_existing().await;
        let result = self.launch(self.locator.find()).await;
        if let Err(error) = &result {
            self.set_state(ConnectionState::failed(error));
        }
        result
    }

    /// One health request (the `daemon_health` command).
    pub async fn health(&self) -> Result<Response, DaemonError> {
        self.client
            .request_with_timeout(&Request::Health, PROBE_TIMEOUT)
            .await
    }

    async fn ensure_now(&self) -> Result<(), DaemonError> {
        let binary = self.locator.find();
        let probe = self.probe_with_retries().await;
        let restart = probe != Probe::Unreachable
            && binary
                .as_ref()
                .is_ok_and(|binary| should_restart(binary, &self.pid_path()));
        if probe != Probe::Unreachable && !restart {
            return self.adopt(probe, false);
        }
        self.stop_existing().await;
        self.launch(binary).await
    }

    /// Take the answering daemon as the connected one, or fail if it is incompatible.
    fn adopt(&self, probe: Probe, launched_now: bool) -> Result<(), DaemonError> {
        let (pid, launched) = match probe {
            Probe::Healthy {
                protocol_version, ..
            } if protocol_version != PROTOCOL_VERSION => {
                return Err(DaemonError::Incompatible {
                    daemon: protocol_version,
                    app: PROTOCOL_VERSION,
                });
            }
            Probe::Healthy { pid, .. } => {
                let mut launched = lock(&self.launched_pid);
                if launched_now {
                    // Ours only if the daemon answering is the one we spawned: if another
                    // instance won the daemon lock, ours exited and we attached to theirs.
                    let ours = *launched == Some(pid);
                    if !ours {
                        *launched = None;
                    }
                }
                (Some(pid), *launched == Some(pid))
            }
            // BUSY: alive, so it cannot be a daemon we just started; attach without owning.
            Probe::Busy => {
                *lock(&self.launched_pid) = None;
                (None, false)
            }
            Probe::Unreachable => return Err(DaemonError::StartupTimeout),
        };
        self.set_state(ConnectionState::Connected { pid, launched });
        Ok(())
    }

    fn pid_path(&self) -> PathBuf {
        pu_transport::Endpoint::for_socket(&self.socket)
            .map(|e| e.pid_path())
            .unwrap_or_else(|_| {
                let mut p = self.socket.as_os_str().to_os_string();
                p.push(".pid");
                p.into()
            })
    }

    async fn probe(&self) -> Probe {
        match self.health().await {
            Ok(Response::HealthReport {
                pid,
                protocol_version,
                ..
            }) => Probe::Healthy {
                pid,
                protocol_version,
            },
            Err(DaemonError::Busy(_)) => Probe::Busy,
            _ => Probe::Unreachable,
        }
    }

    /// Retry a failed check so one slow answer from a busy daemon is not mistaken for a
    /// dead one (which would end its agents).
    async fn probe_with_retries(&self) -> Probe {
        for attempt in 0..3 {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
            let probe = self.probe().await;
            if probe != Probe::Unreachable {
                return probe;
            }
        }
        Probe::Unreachable
    }

    /// Stop the daemon named by the PID file: `Shutdown` first (the clean stop the engine
    /// handles), then `TerminateProcess` after 3 s. The socket is never touched; only the
    /// daemon holding the lock may remove it.
    async fn stop_existing(&self) {
        *lock(&self.launched_pid) = None;
        let pid_path = self.pid_path();
        let pid = std::fs::read_to_string(&pid_path)
            .ok()
            .and_then(|text| text.trim().parse::<u32>().ok())
            .filter(|pid| *pid > 0);
        let Some(pid) = pid.filter(|pid| process::is_engine(*pid)) else {
            let _ = std::fs::remove_file(&pid_path);
            return;
        };
        let _ = self
            .client
            .request_with_timeout(&Request::Shutdown, PROBE_TIMEOUT)
            .await;
        let exited =
            tokio::task::spawn_blocking(move || process::wait_exit(pid, Duration::from_secs(3)))
                .await
                .unwrap_or(false);
        if !exited {
            process::terminate(pid);
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }

    async fn launch(&self, binary: Result<PathBuf, String>) -> Result<(), DaemonError> {
        let binary = binary.map_err(DaemonError::BinaryNotFound)?;
        let pid = spawn_daemon(&binary, &self.socket, self.parent_pid, &self.spawn_env)
            .map_err(|e| DaemonError::Io(format!("Could not start {}: {e}", binary.display())))?;
        *lock(&self.launched_pid) = Some(pid);
        // Backoff 100, 200, 400, 800, 1600 ms (~3 s). If another pu-engine holds the daemon
        // lock, ours exits at once and this attaches to the running one.
        for attempt in 0..5 {
            tokio::time::sleep(Duration::from_millis(100 << attempt)).await;
            let probe = self.probe().await;
            if probe != Probe::Unreachable {
                return self.adopt(probe, true);
            }
        }
        *lock(&self.launched_pid) = None;
        Err(DaemonError::StartupTimeout)
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The Swift rule: restart when the binary was modified after the PID file was written.
/// Missing either date means no restart.
pub fn should_restart(binary: &Path, pid_file: &Path) -> bool {
    let modified = |path: &Path| std::fs::metadata(path).and_then(|m| m.modified()).ok();
    match (modified(binary), modified(pid_file)) {
        (Some(binary), Some(pid)) => binary > pid,
        _ => false,
    }
}

/// Start `binary --managed --parent-pid <parent> --socket <socket>` detached, with no
/// window, stdin and stdout on NUL and stderr appended to `daemon.log`. Returns its PID.
fn spawn_daemon(
    binary: &Path,
    socket: &Path,
    parent_pid: u32,
    env: &[(OsString, OsString)],
) -> std::io::Result<u32> {
    use std::process::{Command, Stdio};
    let log_path = env
        .iter()
        .find(|(key, _)| key == "HOME" || key == "USERPROFILE")
        .map(|(_, home)| PathBuf::from(home).join(".pu").join("daemon.log"))
        .or_else(|| pu_core::paths::daemon_log_path().ok());
    let stderr = log_path
        .and_then(|path| {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .ok()
        })
        .map_or_else(Stdio::null, Stdio::from);
    let socket = std::path::absolute(socket)?;
    let mut command = Command::new(binary);
    command
        .arg("--managed")
        .arg("--parent-pid")
        .arg(parent_pid.to_string())
        .arg("--socket")
        .arg(&socket)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(stderr)
        .envs(env.iter().map(|(k, v)| (k, v)));
    // Never pin the app's working directory: Windows cannot delete a directory that is some
    // process's working directory.
    if let Some(home) = pu_core::paths::home_dir().ok().filter(|h| h.is_dir()) {
        command.current_dir(home);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use windows_sys::Win32::System::Threading::{
            CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW, DETACHED_PROCESS,
        };
        command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
    }
    // Dropping the handle neither waits for nor kills the daemon.
    Ok(command.spawn()?.id())
}

/// The running `pu-engine.exe` named by a PID file.
#[cfg(windows)]
pub(crate) mod process {
    use std::time::Duration;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
        PROCESS_TERMINATE, QueryFullProcessImageNameW, TerminateProcess, WaitForSingleObject,
    };

    struct Process(HANDLE);

    impl Process {
        fn open(pid: u32, access: u32) -> Option<Self> {
            // SAFETY: OpenProcess has no preconditions; null means failure.
            let handle = unsafe { OpenProcess(access, 0, pid) };
            (!handle.is_null()).then_some(Self(handle))
        }
    }

    impl Drop for Process {
        fn drop(&mut self) {
            // SAFETY: a handle we own, closed once.
            unsafe { CloseHandle(self.0) };
        }
    }

    /// True if `pid` is a live `pu-engine.exe`: a PID file outlives a killed daemon, and
    /// the PID may since belong to an unrelated process.
    pub fn is_engine(pid: u32) -> bool {
        let Some(process) = Process::open(pid, PROCESS_QUERY_LIMITED_INFORMATION) else {
            return false;
        };
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        // SAFETY: buf and len describe a writable buffer of len u16s.
        let ok = unsafe {
            QueryFullProcessImageNameW(process.0, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len)
        };
        if ok == 0 {
            return false;
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        path.rsplit('\\')
            .next()
            .is_some_and(|name| name.eq_ignore_ascii_case(pu_core::engine_binary::ENGINE_EXE))
    }

    /// Wait up to `timeout` for `pid` to exit; true once it has (or cannot be opened).
    pub fn wait_exit(pid: u32, timeout: Duration) -> bool {
        let Some(process) = Process::open(pid, PROCESS_SYNCHRONIZE) else {
            return true;
        };
        let ms = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX);
        // SAFETY: a live process handle we own.
        unsafe { WaitForSingleObject(process.0, ms) == WAIT_OBJECT_0 }
    }

    pub fn terminate(pid: u32) {
        if let Some(process) = Process::open(pid, PROCESS_TERMINATE) {
            // SAFETY: a live process handle opened for termination.
            unsafe { TerminateProcess(process.0, 1) };
        }
    }
}

#[cfg(not(windows))]
pub(crate) mod process {
    use std::time::Duration;
    pub fn is_engine(_pid: u32) -> bool {
        false
    }
    pub fn wait_exit(_pid: u32, _timeout: Duration) -> bool {
        true
    }
    pub fn terminate(_pid: u32) {}
}

/// Shared handle for Tauri state.
pub type SharedLifecycle = Arc<DaemonLifecycle>;

#[cfg(test)]
mod tests {
    use super::super::client::tests::{Reply, fake_daemon, health_line};
    use super::*;

    fn lifecycle(socket: &Path) -> DaemonLifecycle {
        // No candidates: nothing may be started, so only reuse is exercised.
        DaemonLifecycle::new(socket, BinaryLocator::default()).unwrap()
    }

    #[tokio::test]
    async fn given_compatible_daemon_should_reuse_it() {
        let tmp = tempfile::TempDir::new().unwrap();
        let socket = tmp.path().join("d.sock");
        let server = fake_daemon(&socket, Reply::Line(health_line(9, PROTOCOL_VERSION)));
        let lifecycle = lifecycle(&socket);
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = seen.clone();
        lifecycle.on_change(move |state| sink.lock().unwrap().push(state.clone()));
        lifecycle.ensure_daemon().await.unwrap();
        let connected = ConnectionState::Connected {
            pid: Some(9),
            launched: false,
        };
        assert_eq!(lifecycle.state(), connected);
        assert_eq!(
            *seen.lock().unwrap(),
            [ConnectionState::Starting, connected]
        );
        server.abort();
    }

    #[tokio::test]
    async fn given_daemon_of_other_protocol_should_report_incompatible() {
        let tmp = tempfile::TempDir::new().unwrap();
        let socket = tmp.path().join("d.sock");
        let server = fake_daemon(&socket, Reply::Line(health_line(9, PROTOCOL_VERSION + 1)));
        let lifecycle = lifecycle(&socket);
        let result = lifecycle.ensure_daemon().await;
        assert!(
            matches!(result, Err(DaemonError::Incompatible { daemon, app }) if daemon == PROTOCOL_VERSION + 1 && app == PROTOCOL_VERSION),
            "{result:?}"
        );
        match lifecycle.state() {
            ConnectionState::Incompatible { message, .. } => assert_eq!(
                message,
                format!(
                    "Incompatible daemon (protocol {}); this app speaks protocol {PROTOCOL_VERSION}. Update PurePoint and restart the daemon after saving active agent work.",
                    PROTOCOL_VERSION + 1
                )
            ),
            other => panic!("expected Incompatible, got {other:?}"),
        }
        server.abort();
    }

    #[tokio::test]
    async fn given_busy_daemon_should_attach_without_owning_it() {
        let tmp = tempfile::TempDir::new().unwrap();
        let socket = tmp.path().join("d.sock");
        let busy = r#"{"type":"error","code":"BUSY","message":"full"}"#;
        let server = fake_daemon(&socket, Reply::Line(busy.into()));
        let lifecycle = lifecycle(&socket);
        lifecycle.ensure_daemon().await.unwrap();
        assert_eq!(
            lifecycle.state(),
            ConnectionState::Connected {
                pid: None,
                launched: false
            }
        );
        server.abort();
    }

    #[tokio::test]
    async fn given_no_daemon_and_no_binary_should_fail_with_banner_text() {
        let tmp = tempfile::TempDir::new().unwrap();
        let lifecycle = lifecycle(&tmp.path().join("d.sock"));
        let result = lifecycle.ensure_daemon().await;
        assert!(
            matches!(result, Err(DaemonError::BinaryNotFound(_))),
            "{result:?}"
        );
        let value = serde_json::to_value(lifecycle.state()).unwrap();
        assert_eq!(value["state"], "failed");
        assert_eq!(value["kind"], "binary_not_found");
        assert_eq!(
            value["message"],
            "Could not find pu-engine.exe (no candidate locations). Reinstall PurePoint or set PU_ENGINE_PATH."
        );
    }

    #[test]
    fn given_binary_newer_than_pid_file_should_restart() {
        let tmp = tempfile::TempDir::new().unwrap();
        let pid = tmp.path().join("d.sock.pid");
        let binary = tmp.path().join("pu-engine.exe");
        std::fs::write(&pid, "1").unwrap();
        std::fs::write(&binary, "MZ").unwrap();
        let older = std::time::SystemTime::now() - Duration::from_secs(60);
        std::fs::File::options()
            .write(true)
            .open(&pid)
            .unwrap()
            .set_modified(older)
            .unwrap();
        assert!(should_restart(&binary, &pid));
        assert!(!should_restart(&pid, &binary));
        assert!(!should_restart(&binary, &tmp.path().join("missing.pid")));
    }

    #[test]
    fn given_states_should_serialize_tagged_camel_case() {
        let value = serde_json::to_value(ConnectionState::Incompatible {
            daemon_version: 3,
            app_version: 14,
            message: "m".into(),
        })
        .unwrap();
        assert_eq!(
            value,
            serde_json::json!({"state": "incompatible", "daemonVersion": 3, "appVersion": 14, "message": "m"})
        );
    }

    /// The real engine built from this repo (`cargo build -p pu-engine` at the root), or
    /// `PU_ENGINE_TEST_BIN`.
    fn built_engine() -> Option<PathBuf> {
        let path = std::env::var_os("PU_ENGINE_TEST_BIN")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../../target/debug")
                    .join(pu_core::engine_binary::ENGINE_EXE)
            });
        path.is_file()
            .then(|| pu_core::canon::canonicalize(&path).unwrap_or(path))
    }

    #[cfg(windows)]
    #[tokio::test(flavor = "multi_thread")]
    async fn given_built_engine_should_start_it_and_report_health() {
        let Some(engine) = built_engine() else {
            eprintln!(
                "skipped: build pu-engine first (`cargo build -p pu-engine` at the repo root)"
            );
            return;
        };
        let home = tempfile::TempDir::new().unwrap();
        let socket = home.path().join(".pu").join("daemon.sock");
        let locator = BinaryLocator {
            dirs: vec![engine.parent().unwrap().to_path_buf()],
            home: None,
            explicit: None,
        };
        let lifecycle = DaemonLifecycle::new(&socket, locator)
            .unwrap()
            .with_spawn_env(vec![
                ("HOME".into(), home.path().into()),
                ("USERPROFILE".into(), home.path().into()),
            ]);
        let started = lifecycle.ensure_daemon().await;
        let log =
            std::fs::read_to_string(home.path().join(".pu").join("daemon.log")).unwrap_or_default();
        started.unwrap_or_else(|e| panic!("{e}; daemon.log:\n{log}"));

        let pid = lifecycle
            .launched_pid()
            .expect("this lifecycle launched the daemon");
        assert_eq!(
            lifecycle.state(),
            ConnectionState::Connected {
                pid: Some(pid),
                launched: true
            }
        );
        match lifecycle.health().await.unwrap() {
            Response::HealthReport {
                pid: reported,
                protocol_version,
                ..
            } => {
                assert_eq!(reported, pid);
                assert_eq!(protocol_version, PROTOCOL_VERSION);
            }
            other => panic!("expected HealthReport, got {other:?}"),
        }
        // A second ensure reuses the running daemon.
        lifecycle.ensure_daemon().await.unwrap();
        assert_eq!(lifecycle.launched_pid(), Some(pid));

        lifecycle
            .client()
            .request(&Request::Shutdown)
            .await
            .unwrap();
        assert!(
            tokio::task::spawn_blocking(move || process::wait_exit(pid, Duration::from_secs(10)))
                .await
                .unwrap(),
            "daemon did not exit after Shutdown"
        );
    }
}

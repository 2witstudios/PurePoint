//! Port of the process side of `State/PointGuardServiceModel.swift`: the app owns exactly the
//! bridge it launched; a descriptor never authorizes adopting or signalling another process.
//!
//! - Start: locate and seal-check the runtime, then run `node.exe <entry> --managed` with no
//!   window, stdout/stderr on NUL, the user's home as working directory and a scrubbed copy
//!   of the app's environment ([`bridge_environment`]). Its stdin is a pipe the app holds:
//!   EOF (Stop for Quit, the app exiting or crashing) makes the bridge drain and exit, the
//!   Windows stand-in for the Mac's SIGTERM.
//! - Ready: the bridge writes the private `admin.json` descriptor; it counts only when it
//!   names this child's pid, this launch's `POINT_GUARD_INSTANCE_ID` and the owner pipe for
//!   the state directory. Polled every 100 ms for 30 s, failing early with the bridge's
//!   `error.json` when the child exits.
//! - Stop: ask the bridge (`runtime.stop`, which drains while idle) and wait for the exact
//!   child to exit; a child that is not ready yet gets EOF instead.
//! - Restart after a crash (a Windows addition: the Mac only offers Retry): a bridge that was
//!   ready and exits unexpectedly is relaunched after 1, 2, 4, 8 then 16 s; five restarts
//!   without a minute of stable running give up and leave the error for Retry. Startup
//!   failures are never retried automatically.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{Map, Value};
use tokio::process::ChildStdin;
use tokio::sync::{oneshot, watch};

use super::admin;
use super::runtime::{self, Descriptor, Layout, Runtime};

/// Environment variable names never passed to the bridge from the app's environment.
pub fn scrubbed(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    upper.starts_with("PU_")
        || upper.starts_with("PI_")
        || upper.starts_with("POINT_GUARD_")
        // Loader and debug hooks (NODE_OPTIONS, NODE_PATH, NODE_DEBUG, NODE_V8_COVERAGE,
        // NODE_TLS_REJECT_UNAUTHORIZED, ...); extra CA certificates stay for corporate proxies.
        || (upper.starts_with("NODE_") && upper != "NODE_EXTRA_CA_CERTS")
        || upper == "PATH"
}

/// What one launch needs besides the runtime.
#[derive(Debug, Clone)]
pub struct Launch {
    pub state_dir: PathBuf,
    pub home: PathBuf,
    pub instance_id: String,
    pub cwd: Option<PathBuf>,
    pub tailnet: Option<String>,
}

/// The bridge's environment: the app's own minus [`scrubbed`] names, plus the launch's.
pub fn bridge_environment(
    base: &[(OsString, OsString)],
    runtime: &Runtime,
    launch: &Launch,
) -> Vec<(OsString, OsString)> {
    let mut env: Vec<(OsString, OsString)> = base
        .iter()
        .filter(|(k, _)| !scrubbed(&k.to_string_lossy()))
        .cloned()
        .collect();
    let system_root = base
        .iter()
        .find(|(k, _)| k.to_string_lossy().eq_ignore_ascii_case("SystemRoot"))
        .map(|(_, v)| v.to_string_lossy().into_owned())
        .unwrap_or_else(|| r"C:\Windows".into());
    let pu_dir = runtime
        .pu
        .parent()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let mut set = |k: &str, v: String| env.push((k.into(), v.into()));
    set(
        "PATH",
        format!(
            "{pu_dir};{system_root}\\System32;{system_root};{system_root}\\System32\\Wbem;{system_root}\\System32\\WindowsPowerShell\\v1.0"
        ),
    );
    set(
        "POINT_GUARD_STATE_DIR",
        launch.state_dir.display().to_string(),
    );
    set("POINT_GUARD_PU_PATH", runtime.pu.display().to_string());
    set(
        "POINT_GUARD_PU_HOME",
        launch.home.join(".pu").display().to_string(),
    );
    set(
        "POINT_GUARD_LOCK_HELPER_PATH",
        runtime.lock_helper.display().to_string(),
    );
    set("POINT_GUARD_INSTANCE_ID", launch.instance_id.clone());
    set("POINT_GUARD_APP_LIFETIME", "stdin".into());
    if let Some(cwd) = &launch.cwd {
        set("PI_MOBILE_CWD", cwd.display().to_string());
    }
    if let Some(host) = &launch.tailnet {
        set("PI_MOBILE_HOST", host.clone());
    }
    env
}

/// Restart delays after a crash, and how long a run must last to reset them.
#[derive(Debug, Clone)]
pub struct Backoff {
    pub delays: Vec<Duration>,
    pub stable_after: Duration,
}

impl Default for Backoff {
    fn default() -> Self {
        Self {
            delays: [1, 2, 4, 8, 16].map(Duration::from_secs).to_vec(),
            stable_after: Duration::from_secs(60),
        }
    }
}

type Locate = Arc<dyn Fn() -> Result<Runtime, String> + Send + Sync>;
type Tailnet = Arc<dyn Fn() -> Option<String> + Send + Sync>;
type Listener = Box<dyn Fn(&Snapshot) + Send + Sync>;

/// Everything the service touches; tests pass temp dirs and fakes.
#[derive(Clone)]
pub struct Config {
    pub state_dir: PathBuf,
    pub home: PathBuf,
    pub locate: Locate,
    pub tailnet: Tailnet,
    /// The environment the bridge's is derived from (the app's own in production).
    pub base_env: Vec<(OsString, OsString)>,
    pub ready_timeout: Duration,
    pub stop_timeout: Duration,
    pub backoff: Backoff,
}

/// What the UI sees (`pointguard-state`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub phase: String,
    pub error: Option<String>,
    pub ready: bool,
    /// An owned child exists (starting, ready or stopping).
    pub running: bool,
    pub instance_id: Option<String>,
    /// The phone endpoint (`chatURL`), when the bridge listens on the tailnet.
    pub remote_endpoint: Option<String>,
    pub layout: Option<Layout>,
    /// Automatic restarts since the last stable run.
    pub restarts: u32,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            phase: "Stopped".into(),
            error: None,
            ready: false,
            running: false,
            instance_id: None,
            remote_endpoint: None,
            layout: None,
            restarts: 0,
        }
    }
}

/// The local chat endpoint and secret for the Point Guard chat (WIN-9.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatConnection {
    pub endpoint: String,
    pub secret: String,
    pub client_id: String,
}

struct Owned {
    generation: u64,
    instance_id: String,
    pid: u32,
    stdin: Option<ChildStdin>,
    kill: Option<oneshot::Sender<()>>,
    exited: watch::Receiver<Option<i32>>,
    descriptor: Option<Descriptor>,
    token: Option<String>,
    ready_at: Option<Instant>,
    expected_stop: bool,
}

#[derive(Default)]
struct Control {
    starting: bool,
    stopping: bool,
    restarts: u32,
}

struct Inner {
    config: Config,
    snapshot: Mutex<Snapshot>,
    owned: Mutex<Option<Owned>>,
    control: Mutex<Control>,
    /// Bumped by every stop: cancels an in-flight start and a pending restart.
    epoch: AtomicU64,
    generation: AtomicU64,
    listener: Mutex<Option<Listener>>,
}

/// The app-owned Point Guard runtime.
#[derive(Clone)]
pub struct PointGuardService {
    inner: Arc<Inner>,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

const NOT_READY: &str = "Start Point Guard before changing setup.";
const SETUP_NEEDS_ATTENTION: &str = "Setup needs attention";

impl PointGuardService {
    pub fn new(config: Config) -> Self {
        Self {
            inner: Arc::new(Inner {
                config,
                snapshot: Mutex::new(Snapshot::default()),
                owned: Mutex::new(None),
                control: Mutex::new(Control::default()),
                epoch: AtomicU64::new(0),
                generation: AtomicU64::new(0),
                listener: Mutex::new(None),
            }),
        }
    }

    pub fn on_change(&self, listener: impl Fn(&Snapshot) + Send + Sync + 'static) {
        *lock(&self.inner.listener) = Some(Box::new(listener));
    }

    pub fn snapshot(&self) -> Snapshot {
        lock(&self.inner.snapshot).clone()
    }

    fn update(&self, change: impl FnOnce(&mut Snapshot)) {
        let snapshot = {
            let mut s = lock(&self.inner.snapshot);
            change(&mut s);
            s.restarts = lock(&self.inner.control).restarts;
            s.clone()
        };
        if let Some(listener) = lock(&self.inner.listener).as_ref() {
            listener(&snapshot);
        }
    }

    /// Show an error from a UI-driven step (the Swift `error` setter).
    pub fn set_error(&self, message: String) {
        self.update(|s| s.error = Some(message));
    }

    /// An owned child exists or a start is in flight (Quit must wait for it).
    pub fn is_active(&self) -> bool {
        let owned = lock(&self.inner.owned).is_some();
        owned || lock(&self.inner.control).starting
    }

    /// The owned child's pid, while there is one.
    pub fn pid(&self) -> Option<u32> {
        lock(&self.inner.owned).as_ref().map(|o| o.pid)
    }

    /// Launch the runtime and wait until it is ready or has failed. A no-op while a child is
    /// owned, starting or stopping. `cwd` is the explicit recovery folder for this launch.
    pub async fn start(&self, cwd: Option<PathBuf>) -> Result<(), String> {
        {
            let mut control = lock(&self.inner.control);
            if control.starting || control.stopping || lock(&self.inner.owned).is_some() {
                return Ok(());
            }
            control.starting = true;
        }
        let epoch = self.inner.epoch.load(Ordering::SeqCst);
        self.update(|s| {
            s.error = None;
            s.phase = "Starting Point Guard…".into();
        });
        let result = self.launch(cwd, epoch).await;
        lock(&self.inner.control).starting = false;
        match result {
            Ok(()) => Ok(()),
            Err(None) => Ok(()), // canceled by stop
            Err(Some(message)) => {
                let killed = {
                    let mut owned = lock(&self.inner.owned);
                    owned.as_mut().map(|o| {
                        o.expected_stop = true;
                        o.stdin = None;
                        o.kill.take()
                    })
                };
                // Only this exact app-owned child; never a descriptor pid.
                if let Some(Some(kill)) = killed {
                    let _ = kill.send(());
                }
                self.update(|s| {
                    s.error = Some(message.clone());
                    s.phase = SETUP_NEEDS_ATTENTION.into();
                    s.ready = false;
                });
                Err(message)
            }
        }
    }

    fn canceled(&self, epoch: u64) -> bool {
        self.inner.epoch.load(Ordering::SeqCst) != epoch
    }

    async fn launch(&self, cwd: Option<PathBuf>, epoch: u64) -> Result<(), Option<String>> {
        let config = self.inner.config.clone();
        let locate = config.locate.clone();
        let runtime = tokio::task::spawn_blocking(move || locate())
            .await
            .map_err(|e| Some(e.to_string()))?
            .map_err(Some)?;
        if self.canceled(epoch) {
            return Err(None);
        }
        let tailnet = config.tailnet.clone();
        let tailnet = tokio::task::spawn_blocking(move || tailnet())
            .await
            .unwrap_or(None);
        if self.canceled(epoch) {
            return Err(None);
        }
        let launch = Launch {
            state_dir: config.state_dir.clone(),
            home: config.home.clone(),
            instance_id: uuid::Uuid::new_v4().to_string(),
            cwd,
            tailnet,
        };
        let mut command = tokio::process::Command::new(&runtime.node);
        command
            .arg(&runtime.entry)
            .arg("--managed")
            .env_clear()
            .envs(bridge_environment(&config.base_env, &runtime, &launch))
            .current_dir(&config.home)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        #[cfg(windows)]
        command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
        let mut child = command
            .spawn()
            .map_err(|e| Some(format!("Point Guard could not start: {e}")))?;
        let pid = child.id().unwrap_or(0);
        let stdin = child.stdin.take();
        let (kill_tx, kill_rx) = oneshot::channel::<()>();
        let (exit_tx, exited) = watch::channel(None);
        let generation = self.inner.generation.fetch_add(1, Ordering::SeqCst) + 1;
        *lock(&self.inner.owned) = Some(Owned {
            generation,
            instance_id: launch.instance_id.clone(),
            pid,
            stdin,
            kill: Some(kill_tx),
            exited: exited.clone(),
            descriptor: None,
            token: None,
            ready_at: None,
            expected_stop: false,
        });
        self.update(|s| {
            s.running = true;
            s.instance_id = Some(launch.instance_id.clone());
            s.layout = Some(runtime.layout);
        });
        let service = self.clone();
        tokio::spawn(async move {
            let mut kill_rx = kill_rx;
            let status = tokio::select! {
                status = child.wait() => status,
                Ok(()) = &mut kill_rx => {
                    let _ = child.start_kill();
                    child.wait().await
                }
            };
            let code = status.ok().and_then(|s| s.code()).unwrap_or(-1);
            let _ = exit_tx.send(Some(code));
            service.handle_exit(generation, code);
        });

        let descriptor = self
            .await_readiness(pid, &launch.instance_id, exited, epoch)
            .await?;
        let token = runtime::read_token(&config.state_dir.join("admin-token")).map_err(Some)?;
        {
            let mut owned = lock(&self.inner.owned);
            match owned.as_mut() {
                Some(o) if o.generation == generation => {
                    o.descriptor = Some(descriptor.clone());
                    o.token = Some(token);
                    o.ready_at = Some(Instant::now());
                }
                _ => return Err(None),
            }
        }
        self.update(|s| {
            s.ready = true;
            s.phase = "Point Guard ready".into();
            s.error = None;
            s.remote_endpoint = descriptor.chat_url.clone();
        });
        Ok(())
    }

    async fn await_readiness(
        &self,
        pid: u32,
        instance_id: &str,
        mut exited: watch::Receiver<Option<i32>>,
        epoch: u64,
    ) -> Result<Descriptor, Option<String>> {
        let state = self.inner.config.state_dir.clone();
        let deadline = Instant::now() + self.inner.config.ready_timeout;
        loop {
            if self.canceled(epoch) {
                return Err(None);
            }
            if exited.borrow().is_some() {
                return Err(Some(
                    runtime::startup_failure(&state, pid, instance_id).unwrap_or_else(|| {
                        "Point Guard could not start. Check state permissions, existing runtime ownership and ports, then Retry.".into()
                    }),
                ));
            }
            let (dir, id) = (state.clone(), instance_id.to_string());
            if let Ok(Some(descriptor)) =
                tokio::task::spawn_blocking(move || runtime::read_descriptor(&dir, pid, &id)).await
            {
                return Ok(descriptor);
            }
            if Instant::now() >= deadline {
                return Err(Some(format!(
                    "Point Guard did not become ready in {} seconds. Retry after checking its private state and ports.",
                    self.inner.config.ready_timeout.as_secs()
                )));
            }
            let _ = tokio::time::timeout(Duration::from_millis(100), exited.changed()).await;
        }
    }

    /// Only the exact owned child's exit clears its state.
    fn handle_exit(&self, generation: u64, code: i32) {
        let owned = {
            let mut slot = lock(&self.inner.owned);
            match slot.as_ref() {
                Some(o) if o.generation == generation => slot.take(),
                _ => None,
            }
        };
        let Some(owned) = owned else { return };
        let unexpected = !owned.expected_stop;
        let failure = unexpected
            .then(|| {
                runtime::startup_failure(
                    &self.inner.config.state_dir,
                    owned.pid,
                    &owned.instance_id,
                )
            })
            .flatten();
        let restart_in = match owned.ready_at {
            Some(ready_at) if unexpected => {
                let mut control = lock(&self.inner.control);
                if ready_at.elapsed() >= self.inner.config.backoff.stable_after {
                    control.restarts = 0;
                }
                let delay = self
                    .inner
                    .config
                    .backoff
                    .delays
                    .get(control.restarts as usize)
                    .copied();
                if delay.is_some() {
                    control.restarts += 1;
                }
                delay
            }
            _ => None,
        };
        self.update(|s| {
            s.ready = false;
            s.running = false;
            s.instance_id = None;
            s.remote_endpoint = None;
            // A failed start keeps its "Setup needs attention" over the exit it caused.
            if s.phase != SETUP_NEEDS_ATTENTION {
                s.phase = "Stopped".into();
            }
            if unexpected {
                s.phase = match restart_in {
                    Some(delay) => format!("Point Guard stopped · restarting in {} s", delay.as_secs()),
                    None => "Point Guard stopped".into(),
                };
                s.error = Some(failure.clone().unwrap_or_else(|| format!(
                    "Point Guard exited ({code}). Retry to restore your saved session. Check private state permissions and available ports if startup fails."
                )));
            }
        });
        if let Some(delay) = restart_in {
            let service = self.clone();
            let epoch = self.inner.epoch.load(Ordering::SeqCst);
            tokio::spawn(async move {
                tokio::time::sleep(delay).await;
                if !service.canceled(epoch) {
                    let _ = service.start(None).await;
                }
            });
        }
    }

    /// One admin operation on the owned, ready bridge (the Swift `request`).
    pub async fn request(
        &self,
        operation: &str,
        fields: Map<String, Value>,
    ) -> Result<Value, String> {
        let (pipe, pid, token, generation) = {
            let owned = lock(&self.inner.owned);
            let o = owned.as_ref().ok_or(NOT_READY)?;
            let (Some(descriptor), Some(token)) = (&o.descriptor, &o.token) else {
                return Err(NOT_READY.into());
            };
            if o.exited.borrow().is_some() {
                return Err(NOT_READY.into());
            }
            (
                descriptor.admin_socket.clone().ok_or(NOT_READY)?,
                o.pid,
                token.clone(),
                o.generation,
            )
        };
        let mut body = fields;
        body.insert("operation".into(), Value::String(operation.into()));
        let result = tokio::task::spawn_blocking(move || {
            admin::call(&pipe, pid, &token, &Value::Object(body))
        })
        .await
        .map_err(|e| e.to_string())??;
        let current = lock(&self.inner.owned).as_ref().map(|o| o.generation);
        if current != Some(generation) && operation != "runtime.stop" {
            return Err(
                "Point Guard setup request failed. Retry after checking the service.".into(),
            );
        }
        Ok(result)
    }

    /// The local chat endpoint and the desktop chat secret, while ready.
    pub fn chat_connection(&self) -> Result<ChatConnection, String> {
        let descriptor = lock(&self.inner.owned)
            .as_ref()
            .and_then(|o| o.descriptor.clone())
            .ok_or(NOT_READY)?;
        let secret = runtime::read_token(&self.inner.config.state_dir.join("desktop-chat-token"))?;
        Ok(ChatConnection {
            endpoint: descriptor.native_chat_url,
            secret,
            client_id: descriptor.desktop_client_id,
        })
    }

    fn begin_stop(&self) -> Result<(), String> {
        let mut control = lock(&self.inner.control);
        if control.stopping {
            return Err("Point Guard is already stopping.".into());
        }
        control.stopping = true;
        control.restarts = 0;
        self.inner.epoch.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn end_stop(&self) {
        lock(&self.inner.control).stopping = false;
    }

    async fn wait_exit(&self, mut exited: watch::Receiver<Option<i32>>) -> bool {
        tokio::time::timeout(self.inner.config.stop_timeout, async {
            while exited.borrow().is_none() {
                if exited.changed().await.is_err() {
                    break;
                }
            }
        })
        .await
        .is_ok()
    }

    /// Stop the owned runtime while idle (the Swift `stop`). Errors leave it running.
    pub async fn stop(&self) -> Result<(), String> {
        self.begin_stop()?;
        let result = self.stop_inner().await;
        self.end_stop();
        result
    }

    async fn stop_inner(&self) -> Result<(), String> {
        let (ready, exited) = {
            let mut owned = lock(&self.inner.owned);
            match owned.as_mut() {
                None => {
                    drop(owned);
                    self.update(|s| s.phase = "Stopped".into());
                    return Ok(());
                }
                Some(o) => {
                    o.expected_stop = true;
                    (o.descriptor.is_some(), o.exited.clone())
                }
            }
        };
        if ready {
            // The bridge freezes new mutations and drains in the same serialized operation.
            if let Err(error) = self.request("runtime.stop", Map::new()).await {
                if let Some(o) = lock(&self.inner.owned).as_mut() {
                    o.expected_stop = false;
                }
                return Err(error);
            }
        } else if let Some(o) = lock(&self.inner.owned).as_mut() {
            o.stdin = None; // Failed or pending startup: EOF to this exact owned child.
        }
        self.update(|s| {
            s.ready = false;
            s.phase = "Stopping Point Guard…".into();
        });
        if self.wait_exit(exited).await {
            self.update(|s| s.phase = "Stopped".into());
            Ok(())
        } else {
            Err("Point Guard has not exited. The update/restart is paused until its owned child stops.".into())
        }
    }

    /// After the Quit prompt's deliberate "Stop Point Guard and Quit": end the run even when
    /// busy, by closing the lifetime pipe (the bridge drains as on the Mac's SIGTERM).
    pub async fn stop_owned_for_quit(&self) -> Result<(), String> {
        self.begin_stop()
            .map_err(|_| "Point Guard is already stopping. Wait, then retry Quit.".to_string())?;
        let exited = {
            let mut owned = lock(&self.inner.owned);
            owned.as_mut().map(|o| {
                o.expected_stop = true;
                o.stdin = None;
                o.exited.clone()
            })
        };
        let result = match exited {
            None => Ok(()),
            Some(exited) => {
                if self.wait_exit(exited).await {
                    Ok(())
                } else {
                    Err("The owned Point Guard runtime has not exited. Quit and updates remain paused. Wait for it to stop, then retry Quit.".into())
                }
            }
        };
        if result.is_ok() {
            self.update(|s| s.phase = "Stopped".into());
        }
        self.end_stop();
        result
    }

    /// Stop, then start again (the Swift `restart`; the UI checks the chat is idle first).
    pub async fn restart(&self, cwd: Option<PathBuf>) -> Result<(), String> {
        self.stop().await?;
        self.start(cwd).await
    }

    /// App exit cannot await: close the lifetime pipe and wait, bounded, for the child.
    pub fn stop_for_termination(&self) {
        self.inner.epoch.fetch_add(1, Ordering::SeqCst);
        let exited = {
            let mut owned = lock(&self.inner.owned);
            owned.as_mut().map(|o| {
                o.expected_stop = true;
                o.stdin = None;
                o.exited.clone()
            })
        };
        let Some(exited) = exited else { return };
        let deadline = Instant::now() + Duration::from_secs(10);
        while exited.borrow().is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

/// The production configuration for this user.
pub fn app_config(
    resource_dir: Option<PathBuf>,
    pu_dirs: Vec<PathBuf>,
    home: PathBuf,
    state_dir: PathBuf,
) -> Config {
    let dev = std::env::var(runtime::DEV_ENV).is_ok_and(|v| v == "1");
    let locate_home = home.clone();
    Config {
        state_dir,
        home,
        locate: Arc::new(move || {
            runtime::locate(resource_dir.as_deref(), &pu_dirs, Some(&locate_home), dev)
        }),
        tailnet: Arc::new(super::tailnet::address),
        base_env: std::env::vars_os().collect(),
        ready_timeout: Duration::from_secs(30),
        stop_timeout: Duration::from_secs(10),
        backoff: Backoff::default(),
    }
}

/// `%LOCALAPPDATA%\PurePoint\point-guard`, where the bridge keeps its state (WIN-9.1).
pub fn default_state_dir(home: &Path) -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home.join("AppData").join("Local"))
        .join("PurePoint")
        .join("point-guard")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runtime() -> Runtime {
        Runtime {
            node: r"C:\rt\bin\node.exe".into(),
            entry: r"C:\rt\bridge\main.js".into(),
            lock_helper: r"C:\rt\bin\point-guard-lock.exe".into(),
            pu: r"C:\app\pu.exe".into(),
            layout: Layout::Bundled,
        }
    }

    fn get<'a>(env: &'a [(OsString, OsString)], key: &str) -> Option<&'a str> {
        env.iter()
            .find(|(k, _)| k.to_string_lossy().eq_ignore_ascii_case(key))
            .and_then(|(_, v)| v.to_str())
    }

    #[test]
    fn given_app_environment_should_scrub_pu_pi_and_node_hooks() {
        let base: Vec<(OsString, OsString)> = [
            ("PU_AGENT_ID", "a"),
            ("pu_project_root", "r"),
            ("PI_MOBILE_TOKEN_FILE", "t"),
            ("PI_MOBILE_HOST", "1.2.3.4"),
            ("NODE_OPTIONS", "--require evil"),
            ("NODE_PATH", "x"),
            ("NODE_DEBUG", "net"),
            ("NODE_TLS_REJECT_UNAUTHORIZED", "0"),
            ("NODE_EXTRA_CA_CERTS", r"C:\ca.pem"),
            ("POINT_GUARD_SUPERVISOR", "systemd"),
            ("Path", r"C:\evil"),
            ("SystemRoot", r"C:\Windows"),
            ("USERPROFILE", r"C:\Users\u"),
        ]
        .into_iter()
        .map(|(k, v)| (k.into(), v.into()))
        .collect();
        let launch = Launch {
            state_dir: r"C:\state".into(),
            home: r"C:\Users\u".into(),
            instance_id: "id".into(),
            cwd: Some(r"C:\work".into()),
            tailnet: None,
        };
        let env = bridge_environment(&base, &runtime(), &launch);
        for gone in [
            "PU_AGENT_ID",
            "PU_PROJECT_ROOT",
            "PI_MOBILE_TOKEN_FILE",
            "PI_MOBILE_HOST",
            "NODE_OPTIONS",
            "NODE_PATH",
            "NODE_DEBUG",
            "NODE_TLS_REJECT_UNAUTHORIZED",
            "POINT_GUARD_SUPERVISOR",
        ] {
            assert_eq!(get(&env, gone), None, "{gone}");
        }
        assert_eq!(get(&env, "NODE_EXTRA_CA_CERTS"), Some(r"C:\ca.pem"));
        assert_eq!(get(&env, "USERPROFILE"), Some(r"C:\Users\u"));
        assert_eq!(
            get(&env, "PATH"),
            Some(
                r"C:\app;C:\Windows\System32;C:\Windows;C:\Windows\System32\Wbem;C:\Windows\System32\WindowsPowerShell\v1.0"
            )
        );
        assert_eq!(
            env.iter()
                .filter(|(k, _)| k.eq_ignore_ascii_case("path"))
                .count(),
            1
        );
        assert_eq!(get(&env, "POINT_GUARD_STATE_DIR"), Some(r"C:\state"));
        assert_eq!(get(&env, "POINT_GUARD_PU_PATH"), Some(r"C:\app\pu.exe"));
        assert_eq!(get(&env, "POINT_GUARD_PU_HOME"), Some(r"C:\Users\u\.pu"));
        assert_eq!(
            get(&env, "POINT_GUARD_LOCK_HELPER_PATH"),
            Some(r"C:\rt\bin\point-guard-lock.exe")
        );
        assert_eq!(get(&env, "POINT_GUARD_INSTANCE_ID"), Some("id"));
        assert_eq!(get(&env, "POINT_GUARD_APP_LIFETIME"), Some("stdin"));
        assert_eq!(get(&env, "PI_MOBILE_CWD"), Some(r"C:\work"));
        let tailnet = Launch {
            tailnet: Some("100.100.1.2".into()),
            cwd: None,
            ..launch
        };
        let env = bridge_environment(&base, &runtime(), &tailnet);
        assert_eq!(get(&env, "PI_MOBILE_HOST"), Some("100.100.1.2"));
        assert_eq!(get(&env, "PI_MOBILE_CWD"), None);
    }

    #[test]
    fn given_no_localappdata_should_fall_back_under_home() {
        let dir = default_state_dir(Path::new(r"C:\Users\u"));
        assert!(dir.ends_with(r"PurePoint\point-guard"));
    }
}

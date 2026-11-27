//! WIN-9.3: the app-owned Point Guard runtime against a fake bridge
//! (`fixtures/pointguard/fake-bridge.mjs`) that follows the real readiness handshake.
//!
//! Every test runs in a temp profile: the child's environment is built only from the
//! config's `base_env`, whose HOME, USERPROFILE, LOCALAPPDATA, APPDATA and TEMP point into
//! the test's temp dir. Tests skip when `node` is not on PATH.
#![cfg(windows)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use purepoint_windows_lib::pointguard::lifecycle::{self, QuitPrompt};
use purepoint_windows_lib::pointguard::runtime::{self, Layout, Runtime};
use purepoint_windows_lib::pointguard::seal;
use purepoint_windows_lib::pointguard::service::{Backoff, Config, PointGuardService, Snapshot};
use serde_json::{Map, Value, json};

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn fake_bridge() -> PathBuf {
    manifest_dir().join("tests/fixtures/pointguard/fake-bridge.mjs")
}

fn platform_url() -> String {
    let platform = manifest_dir().join("../../purepoint-mobile/bridge/platform.js");
    let path = pu_core::canon::canonicalize(platform).unwrap();
    format!("file:///{}", path.display().to_string().replace('\\', "/"))
}

fn node() -> Option<PathBuf> {
    which::which("node").ok()
}

/// A temp profile: home, %LOCALAPPDATA%, %APPDATA%, %TEMP% and the private state directory.
struct Profile {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
    state: PathBuf,
    base_env: Vec<(OsString, OsString)>,
}

impl Profile {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let root = pu_core::canon::canonicalize(tmp.path()).unwrap();
        let home = root.join("home");
        let local = home.join(r"AppData\Local");
        let roaming = home.join(r"AppData\Roaming");
        let temp = root.join("temp");
        for dir in [&home, &local, &roaming, &temp] {
            std::fs::create_dir_all(dir).unwrap();
        }
        let state = local.join(r"PurePoint\point-guard");
        std::fs::create_dir_all(&state).unwrap();
        pu_core::win_security::set_private_dacl(&state, true).unwrap();
        let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        let mut base_env: Vec<(OsString, OsString)> = vec![
            ("SystemRoot".into(), system_root.into()),
            ("HOME".into(), home.clone().into()),
            ("USERPROFILE".into(), home.clone().into()),
            ("LOCALAPPDATA".into(), local.into()),
            ("APPDATA".into(), roaming.into()),
            ("TEMP".into(), temp.clone().into()),
            ("TMP".into(), temp.into()),
            ("FAKE_PLATFORM_URL".into(), platform_url().into()),
        ];
        for key in ["USERNAME", "COMPUTERNAME", "PATHEXT", "ComSpec"] {
            if let Some(value) = std::env::var_os(key) {
                base_env.push((key.into(), value));
            }
        }
        Self {
            _tmp: tmp,
            root,
            home,
            state,
            base_env,
        }
    }

    fn with(mut self, key: &str, value: impl Into<OsString>) -> Self {
        self.base_env.push((key.into(), value.into()));
        self
    }

    fn runtime(&self, node: &Path) -> Runtime {
        let bin = self.root.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        for file in ["pu.exe", "pu-point-guard-lock.exe"] {
            std::fs::write(bin.join(file), "stub").unwrap();
        }
        Runtime {
            node: node.to_path_buf(),
            entry: fake_bridge(),
            lock_helper: bin.join("pu-point-guard-lock.exe"),
            pu: bin.join("pu.exe"),
            layout: Layout::Development,
        }
    }

    fn config(&self, runtime: Result<Runtime, String>) -> Config {
        Config {
            state_dir: self.state.clone(),
            home: self.home.clone(),
            locate: Arc::new(move || runtime.clone()),
            tailnet: Arc::new(|| None),
            base_env: self.base_env.clone(),
            ready_timeout: Duration::from_secs(20),
            stop_timeout: Duration::from_secs(10),
            backoff: Backoff {
                delays: vec![Duration::from_millis(500); 2],
                stable_after: Duration::from_secs(60),
            },
        }
    }

    fn service(&self, node: &Path) -> PointGuardService {
        PointGuardService::new(self.config(Ok(self.runtime(node))))
    }
}

/// Every snapshot the service published.
fn record(service: &PointGuardService) -> Arc<Mutex<Vec<Snapshot>>> {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = seen.clone();
    service.on_change(move |s| sink.lock().unwrap().push(s.clone()));
    seen
}

fn alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, WAIT_TIMEOUT};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
    };
    // SAFETY: plain handle open/wait/close.
    unsafe {
        let handle = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if handle.is_null() {
            return false;
        }
        let running = WaitForSingleObject(handle, 0) == WAIT_TIMEOUT;
        CloseHandle(handle);
        running
    }
}

async fn wait_for(service: &PointGuardService, what: impl Fn(&Snapshot) -> bool) -> Snapshot {
    for _ in 0..200 {
        let s = service.snapshot();
        if what(&s) {
            return s;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("timed out; last snapshot {:?}", service.snapshot());
}

#[tokio::test(flavor = "multi_thread")]
async fn given_fake_bridge_should_reach_ready_answer_admin_and_stop() {
    let Some(node) = node() else { return };
    let profile = Profile::new();
    let service = profile.service(&node);
    let seen = record(&service);
    let cwd = profile.root.join("work");
    std::fs::create_dir_all(&cwd).unwrap();
    service.start(Some(cwd.clone())).await.unwrap();
    let ready = service.snapshot();
    assert!(ready.ready && ready.running, "{ready:?}");
    assert_eq!(ready.phase, "Point Guard ready");
    assert_eq!(ready.layout, Some(Layout::Development));
    assert!(ready.instance_id.is_some());
    assert_eq!(
        seen.lock().unwrap().first().map(|s| s.phase.as_str()),
        Some("Starting Point Guard…")
    );

    let status = service.request("status", Map::new()).await.unwrap();
    assert_eq!(status["cwd"], json!(cwd.display().to_string()));
    let mut fields = Map::new();
    fields.insert("provider".into(), json!("x"));
    let echo = service.request("echo", fields).await.unwrap();
    assert_eq!(echo, json!({"operation": "echo", "provider": "x"}));
    assert_eq!(
        service.request("nope", Map::new()).await.unwrap_err(),
        "Unknown administrative operation."
    );
    let chat = service.chat_connection().unwrap();
    assert_eq!(chat.endpoint, "ws://127.0.0.1:49152/v1");
    assert!(chat.secret.len() >= 32);

    // A second start while owned is a no-op.
    let pid = service.pid().unwrap();
    service.start(None).await.unwrap();
    assert_eq!(service.pid(), Some(pid));

    service.stop().await.unwrap();
    let stopped = wait_for(&service, |s| !s.running).await;
    assert_eq!(stopped.phase, "Stopped");
    assert_eq!(stopped.error, None);
    assert!(!alive(pid));
    assert!(!profile.state.join("admin.json").exists());
    assert_eq!(
        service.request("status", Map::new()).await.unwrap_err(),
        "Start Point Guard before changing setup."
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn given_app_environment_should_launch_with_a_scrubbed_one() {
    let Some(node) = node() else { return };
    let profile = Profile::new();
    let out = profile.root.join("env.json");
    let profile = profile
        .with("FAKE_ENV_OUT", out.clone())
        .with("PU_AGENT_ID", "agent")
        .with("PI_MOBILE_TOKEN_FILE", r"C:\stolen")
        .with("PI_CODING_AGENT_DIR", r"C:\elsewhere")
        .with("NODE_OPTIONS", r"--require C:\does-not-exist.js")
        .with("NODE_DEBUG", "net")
        .with("POINT_GUARD_SUPERVISOR", "systemd")
        .with("Path", r"C:\evil");
    let service = profile.service(&node);
    service.start(None).await.unwrap();
    let dumped: Value = serde_json::from_slice(&std::fs::read(&out).unwrap()).unwrap();
    service.stop().await.unwrap();
    assert_eq!(dumped["argv"], json!(["--managed"]));
    let env = dumped["env"].as_object().unwrap();
    let get = |key: &str| {
        env.iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .and_then(|(_, v)| v.as_str())
    };
    for gone in [
        "PU_AGENT_ID",
        "PI_MOBILE_TOKEN_FILE",
        "PI_CODING_AGENT_DIR",
        "NODE_OPTIONS",
        "NODE_DEBUG",
        "POINT_GUARD_SUPERVISOR",
        "PI_MOBILE_CWD",
        "PI_MOBILE_HOST",
    ] {
        assert_eq!(get(gone), None, "{gone}");
    }
    assert_eq!(get("POINT_GUARD_APP_LIFETIME"), Some("stdin"));
    assert_eq!(
        get("POINT_GUARD_STATE_DIR"),
        Some(profile.state.display().to_string().as_str())
    );
    assert_eq!(
        get("USERPROFILE"),
        Some(profile.home.display().to_string().as_str())
    );
    assert!(!get("PATH").unwrap().contains("evil"));
    assert!(uuid::Uuid::parse_str(get("POINT_GUARD_INSTANCE_ID").unwrap()).is_ok());
}

#[tokio::test(flavor = "multi_thread")]
async fn given_startup_failure_should_report_the_bridge_error_without_retrying() {
    let Some(node) = node() else { return };
    let profile = Profile::new().with("FAKE_MODE", "crash");
    let service = profile.service(&node);
    let error = service.start(None).await.unwrap_err();
    assert_eq!(error, "Fake startup failure. Fix the fake.");
    tokio::time::sleep(Duration::from_millis(400)).await;
    let s = service.snapshot();
    assert_eq!(s.phase, "Setup needs attention");
    assert_eq!(s.error.as_deref(), Some(error.as_str()));
    assert!(!s.running && !s.ready);
    assert_eq!(s.restarts, 0);
    assert_eq!(service.pid(), None);
}

#[tokio::test(flavor = "multi_thread")]
async fn given_bridge_never_ready_should_time_out_and_end_its_child() {
    let Some(node) = node() else { return };
    let profile = Profile::new().with("FAKE_MODE", "hang");
    let mut config = profile.config(Ok(profile.runtime(&node)));
    config.ready_timeout = Duration::from_secs(2);
    let service = PointGuardService::new(config);
    let started = service.clone();
    let task = tokio::spawn(async move { started.start(None).await });
    let running = wait_for(&service, |s| s.running).await;
    assert!(!running.ready);
    let pid = service.pid().unwrap();
    let error = task.await.unwrap().unwrap_err();
    assert!(
        error.contains("did not become ready in 2 seconds"),
        "{error}"
    );
    wait_for(&service, |s| !s.running).await;
    assert!(!alive(pid));
    assert_eq!(service.snapshot().phase, "Setup needs attention");
}

#[tokio::test(flavor = "multi_thread")]
async fn given_crash_after_ready_should_restart_with_backoff() {
    let Some(node) = node() else { return };
    let profile = Profile::new();
    let marker = profile.root.join("crashed-once");
    let profile = profile.with("FAKE_CRASH_ONCE", marker.clone());
    let service = profile.service(&node);
    service.start(None).await.unwrap();
    let first = service.snapshot().instance_id.unwrap();
    let crashed = wait_for(&service, |s| !s.running).await;
    assert_eq!(crashed.restarts, 1);
    assert!(
        crashed
            .phase
            .starts_with("Point Guard stopped · restarting in"),
        "{crashed:?}"
    );
    assert!(
        crashed
            .error
            .unwrap()
            .starts_with("Point Guard exited (7).")
    );
    let again = wait_for(&service, |s| s.ready).await;
    assert_ne!(again.instance_id.as_deref(), Some(first.as_str()));
    assert_eq!(again.error, None);
    assert!(marker.exists());
    service.stop().await.unwrap();
    assert_eq!(service.snapshot().restarts, 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn given_restart_should_replace_the_owned_child() {
    let Some(node) = node() else { return };
    let profile = Profile::new();
    let service = profile.service(&node);
    service.start(None).await.unwrap();
    let (pid, id) = (service.pid().unwrap(), service.snapshot().instance_id);
    service.restart(None).await.unwrap();
    assert!(service.snapshot().ready);
    assert_ne!(service.pid(), Some(pid));
    assert_ne!(service.snapshot().instance_id, id);
    assert!(!alive(pid));
    service.stop().await.unwrap();
}

struct Prompt {
    confirm: bool,
    asked: Mutex<Vec<String>>,
}

impl QuitPrompt for Prompt {
    fn confirm_stop(&self, message: &str) -> bool {
        self.asked.lock().unwrap().push(message.to_string());
        self.confirm
    }
    fn paused(&self, message: &str) {
        self.asked
            .lock()
            .unwrap()
            .push(format!("paused: {message}"));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn given_busy_bridge_quit_should_need_the_deliberate_choice() {
    let Some(node) = node() else { return };
    let profile = Profile::new().with("FAKE_BUSY", "1");
    let service = profile.service(&node);
    service.start(None).await.unwrap();
    let pid = service.pid().unwrap();

    let cancel = Arc::new(Prompt {
        confirm: false,
        asked: Mutex::new(Vec::new()),
    });
    assert!(!lifecycle::may_terminate(&service, cancel.clone()).await);
    assert!(cancel.asked.lock().unwrap()[0].starts_with("Point Guard is busy."));
    assert!(service.snapshot().ready && alive(pid));
    assert_eq!(
        service.snapshot().error.as_deref(),
        Some("Point Guard is busy.")
    );

    let confirm = Arc::new(Prompt {
        confirm: true,
        asked: Mutex::new(Vec::new()),
    });
    assert!(lifecycle::may_terminate(&service, confirm.clone()).await);
    assert_eq!(confirm.asked.lock().unwrap().len(), 1);
    wait_for(&service, |s| !s.running).await;
    assert!(!alive(pid));
    assert_eq!(service.snapshot().phase, "Stopped");
}

#[tokio::test(flavor = "multi_thread")]
async fn given_idle_bridge_quit_should_stop_without_asking() {
    let Some(node) = node() else { return };
    let profile = Profile::new();
    let service = profile.service(&node);
    service.start(None).await.unwrap();
    let prompt = Arc::new(Prompt {
        confirm: false,
        asked: Mutex::new(Vec::new()),
    });
    assert!(lifecycle::may_terminate(&service, prompt.clone()).await);
    assert!(prompt.asked.lock().unwrap().is_empty());
    assert!(!service.is_active());
}

#[tokio::test(flavor = "multi_thread")]
async fn given_app_exit_should_end_the_child_through_its_lifetime_pipe() {
    let Some(node) = node() else { return };
    let profile = Profile::new().with("FAKE_BUSY", "1");
    let service = profile.service(&node);
    service.start(None).await.unwrap();
    let pid = service.pid().unwrap();
    let exiting = service.clone();
    tokio::task::spawn_blocking(move || exiting.stop_for_termination())
        .await
        .unwrap();
    assert!(!alive(pid));
}

/// Stage a bundled runtime: node.exe, the fake as bridge\main.js and the files the manifest
/// names, sealed by the real staging script.
fn stage_bundle(root: &Path, node: &Path) -> PathBuf {
    let dir = root.join(r"resources\PointGuard");
    for file in [
        "bin/point-guard-lock.exe",
        "docs/point-guard.md",
        "support/pu/SKILL.md",
        "support/pu-cli/SKILL.md",
    ] {
        let path = dir.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, file).unwrap();
    }
    // A copy, never a hard link: the binaries get an install's owner-only DACL below, and a link
    // would share (and change) the source's. The runtime refuses binaries other users can write,
    // and a CI runner's tool cache, where `node` comes from, grants that.
    std::fs::copy(node, dir.join(r"bin\node.exe")).unwrap();
    for binary in [r"bin\node.exe", r"bin\point-guard-lock.exe"] {
        pu_core::win_security::set_private_dacl(&dir.join(binary), false).unwrap();
    }
    std::fs::create_dir_all(dir.join("bridge")).unwrap();
    std::fs::copy(fake_bridge(), dir.join(r"bridge\main.js")).unwrap();
    std::fs::write(
        dir.join(seal::MANIFEST),
        json!({
            "schemaVersion": 1, "contractVersion": 1, "layout": "windows",
            "puVersion": "0.1.0", "piVersion": runtime::PI_VERSION, "nodeVersion": "fixture",
            "architecture": runtime::app_architecture(), "sourceSHA": "fixture",
            "paths": {
                "node": "bin/node.exe", "lockHelper": "bin/point-guard-lock.exe",
                "entry": "bridge/main.js", "instructions": "docs/point-guard.md",
                "skills": ["support/pu/SKILL.md", "support/pu-cli/SKILL.md"],
            },
        })
        .to_string(),
    )
    .unwrap();
    let script = manifest_dir().join("../scripts/stage-pointguard-runtime.mjs");
    let status = std::process::Command::new(node)
        .arg(script)
        .arg("--seal")
        .arg(&dir)
        .env_remove("NODE_OPTIONS")
        .stdout(std::process::Stdio::null())
        .status()
        .unwrap();
    assert!(status.success());
    dir
}

#[tokio::test(flavor = "multi_thread")]
async fn given_sealed_bundle_should_launch_and_modified_bundle_should_be_refused() {
    let Some(node) = node() else { return };
    let profile = Profile::new();
    let bundle = stage_bundle(&profile.root, &node);
    let resources = bundle.parent().unwrap().to_path_buf();
    let pu_dir = profile.root.join("app");
    std::fs::create_dir_all(&pu_dir).unwrap();
    std::fs::write(pu_dir.join("pu.exe"), "stub").unwrap();
    let locate = {
        let (resources, pu_dir) = (resources.clone(), pu_dir.clone());
        move || runtime::locate(Some(&resources), std::slice::from_ref(&pu_dir), None, false)
    };
    let runtime = locate().unwrap();
    assert_eq!(runtime.layout, Layout::Bundled);
    assert_eq!(runtime.pu, pu_dir.join("pu.exe"));
    let service = PointGuardService::new(profile.config(Ok(runtime)));
    service.start(None).await.unwrap();
    assert_eq!(service.snapshot().layout, Some(Layout::Bundled));
    service.stop().await.unwrap();

    // One changed byte in the bridge: refused before anything runs.
    let main = bundle.join(r"bridge\main.js");
    let mut text = std::fs::read_to_string(&main).unwrap();
    text.push(' ');
    std::fs::write(&main, text).unwrap();
    let refused = locate().unwrap_err();
    assert_eq!(refused, seal::MODIFIED_MESSAGE);
    let service = PointGuardService::new(profile.config(Err(refused.clone())));
    assert_eq!(service.start(None).await.unwrap_err(), refused);
    let s = service.snapshot();
    assert_eq!(s.phase, "Setup needs attention");
    assert!(!s.running);
    assert_eq!(service.pid(), None);

    // An extra file is refused too.
    let mut original = std::fs::read_to_string(&main).unwrap();
    original.pop();
    std::fs::write(&main, original).unwrap();
    assert!(locate().is_ok());
    std::fs::write(bundle.join(r"bridge\injected.js"), "evil()").unwrap();
    assert_eq!(locate().unwrap_err(), seal::MODIFIED_MESSAGE);
}

/// The manual run (WIN-9.3 criterion 5): the real bridge from the repo, debug layout, in a
/// temp profile. Needs `cargo build -p pu-cli` at the repo root and the mobile app's
/// production `node_modules`; run with `PUREPOINT_POINTGUARD_REAL=1 cargo test --test
/// pointguard_service real_bridge -- --ignored --nocapture`.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "manual: runs the real bridge"]
async fn real_bridge_in_a_temp_profile_reaches_ready_and_stops() {
    if std::env::var("PUREPOINT_POINTGUARD_REAL").as_deref() != Ok("1") {
        return;
    }
    let profile = Profile::new();
    let repo = pu_core::canon::canonicalize(runtime::repo_root()).unwrap();
    // PUREPOINT_POINTGUARD_RESOURCES: a dir holding a payload staged by
    // scripts/stage-pointguard-runtime.mjs, run bundled (sealed) instead.
    let runtime = match std::env::var_os("PUREPOINT_POINTGUARD_RESOURCES") {
        Some(resources) => {
            let pu_dir = repo.join(r"target\debug");
            let sealed = std::time::Instant::now();
            let runtime =
                runtime::locate(Some(Path::new(&resources)), &[pu_dir], None, false).unwrap();
            println!("seal verified in {:?}", sealed.elapsed());
            runtime
        }
        None => runtime::development(&repo).unwrap(),
    };
    let mut config = profile.config(Ok(runtime));
    config.ready_timeout = Duration::from_secs(60);
    config.stop_timeout = Duration::from_secs(30);
    let service = PointGuardService::new(config);
    let seen = record(&service);
    let started = std::time::Instant::now();
    let result = service.start(None).await;
    println!("start: {result:?} after {:?}", started.elapsed());
    println!("snapshot: {:?}", service.snapshot());
    if result.is_ok() {
        let status = service.request("status", Map::new()).await;
        println!("status: {status:?}");
        let stopped = std::time::Instant::now();
        println!(
            "stop: {:?} after {:?}",
            service.stop().await,
            stopped.elapsed()
        );
    }
    println!(
        "phases: {:?}",
        seen.lock()
            .unwrap()
            .iter()
            .map(|s| s.phase.clone())
            .collect::<Vec<_>>()
    );
    println!("final: {:?}", service.snapshot());
    assert!(result.is_ok());
    assert!(!service.is_active());
}

//! `pu --machine` and `pu machine` on Windows, where every daemon connection
//! is an `ssh.exe` exec relay (MACH-012). A fake ssh (`PU_SSH`, a batch file)
//! answers the `uname -s` probe and runs the relay command by starting this
//! build's `pu daemon proxy` against a temp-HOME "remote" daemon.
//!
//! `given_localhost_sshd_*` uses the real Win32-OpenSSH client and server and
//! skips, with the reason on stderr, when `sshd` is absent or refuses a
//! non-interactive key login.
#![cfg(windows)]

use std::path::{Path, PathBuf};

const FAKE_SSH: &str = r#"@echo off
setlocal
echo %*>>"%FAKE_SSH_LOG%"
if defined FAKE_SSH_FAIL (
  echo ssh: Could not resolve hostname fakehost 1>&2
  exit /b 255
)
echo %* | findstr /c:"uname -s" >nul && goto uname
echo %* | findstr /c:"daemon proxy" >nul && goto proxy
echo %* | findstr /c:"pu health" >nul && goto health
echo unexpected ssh invocation: %* 1>&2
exit /b 255
:uname
if "%FAKE_REMOTE_OS%"=="unix" (
  echo Linux
  exit /b 0
)
echo 'uname' is not recognized as an internal or external command 1>&2
exit /b 1
:proxy
if defined FAKE_NO_PU exit /b 9009
"%FAKE_PU%" --socket "%FAKE_REMOTE_SOCKET%" daemon proxy
exit /b %ERRORLEVEL%
:health
echo remote daemon started
exit /b 0
"#;

struct Env {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    local_home: PathBuf,
    remote_socket: PathBuf,
    ssh: PathBuf,
    runs: std::sync::atomic::AtomicUsize,
}

impl Env {
    fn new() -> Self {
        let tmp = tempfile::Builder::new().prefix("pum").tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        let local_home = root.join("h");
        let remote_home = root.join("r");
        std::fs::create_dir_all(&local_home).unwrap();
        std::fs::create_dir_all(remote_home.join(".pu")).unwrap();
        let ssh = root.join("fake-ssh.cmd");
        std::fs::write(&ssh, FAKE_SSH.replace('\n', "\r\n")).unwrap();
        Env {
            remote_socket: remote_home.join(".pu").join("daemon.sock"),
            _tmp: tmp,
            root,
            local_home,
            ssh,
            runs: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    async fn pu(&self, args: &[&str], extra_env: &[(&str, &str)]) -> std::process::Output {
        let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_pu"));
        command
            .args(args)
            .current_dir(&self.root)
            .env("HOME", &self.local_home)
            .env("PU_SSH", &self.ssh)
            .env("FAKE_SSH_LOG", self.root.join("ssh.log"))
            .env("FAKE_PU", env!("CARGO_BIN_EXE_pu"))
            .env("FAKE_REMOTE_SOCKET", &self.remote_socket)
            .env_remove("PU_PROJECT_ROOT")
            .env_remove("PU_AGENT_ID")
            .env_remove("FAKE_SSH_FAIL")
            .env_remove("FAKE_REMOTE_OS")
            .env_remove("FAKE_NO_PU");
        for (key, value) in extra_env {
            command.env(key, value);
        }
        // Files, not pipes: killing the fake ssh (cmd.exe) orphans its
        // `pu daemon proxy`, which inherits every inheritable handle of the
        // CLI, its stdout pipe included, so a pipe would never reach EOF.
        // Real ssh.exe has no such child.
        let n = self.runs.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let out_path = self.root.join(format!("out-{n}.txt"));
        let err_path = self.root.join(format!("err-{n}.txt"));
        command
            .stdin(std::process::Stdio::null())
            .stdout(std::fs::File::create(&out_path).unwrap())
            .stderr(std::fs::File::create(&err_path).unwrap())
            .kill_on_drop(true);
        let status = tokio::time::timeout(std::time::Duration::from_secs(90), command.status())
            .await
            .unwrap_or_else(|_| panic!("pu {args:?} did not finish within 90s"))
            .unwrap();
        std::process::Output {
            status,
            stdout: std::fs::read(&out_path).unwrap(),
            stderr: std::fs::read(&err_path).unwrap(),
        }
    }

    async fn ok_json(&self, args: &[&str], extra_env: &[(&str, &str)]) -> serde_json::Value {
        let output = self.pu(args, extra_env).await;
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }

    async fn add_studio(&self) -> String {
        let added = self
            .ok_json(&["machine", "add", "studio", "fakehost", "--json"], &[])
            .await;
        added["id"].as_str().unwrap().to_string()
    }

    /// The fake's argv lines, without the quotes Rust adds for cmd.exe.
    fn ssh_log(&self) -> String {
        std::fs::read_to_string(self.root.join("ssh.log"))
            .unwrap_or_default()
            .replace('"', "")
    }

    fn machine_dir_entries(&self, machine_id: &str) -> Vec<String> {
        let dir = self
            .local_home
            .join(".pu")
            .join("machines")
            .join(machine_id);
        std::fs::read_dir(dir)
            .map(|entries| {
                entries
                    .flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// A real daemon on the "remote" socket.
fn serve_remote_daemon(socket: &Path) -> tokio::task::JoinHandle<()> {
    let registry = pu_core::paths::project_registry_path(socket);
    let engine = pu_engine::engine::Engine::with_project_registry(registry).unwrap();
    let server = pu_engine::ipc_server::IpcServer::bind(socket, engine).unwrap();
    tokio::spawn(async move {
        server.run().await.unwrap();
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_windows_machine_should_relay_commands_through_pu_daemon_proxy() {
    let env = Env::new();
    let id = env.add_studio().await;

    // No daemon yet: the relay exits 69, which is "stopped", never a start.
    let state = env
        .ok_json(&["machine", "status", "studio", "--json"], &[])
        .await;
    assert_eq!(state[0]["status"], "stopped", "{state}");
    let output = env
        .pu(&["--machine", "studio", "agents", "list", "--json"], &[])
        .await;
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(
        error.contains("daemon not running on studio") && error.contains("pu machine start studio"),
        "{error}"
    );

    let _server = serve_remote_daemon(&env.remote_socket);
    let project = env.root.join("remote-project");
    std::fs::create_dir_all(&project).unwrap();
    let project_s = project.to_str().unwrap();
    env.ok_json(
        &[
            "--socket",
            env.remote_socket.to_str().unwrap(),
            "--project",
            project_s,
            "init",
            "--json",
        ],
        &[],
    )
    .await;

    let state = env
        .ok_json(&["machine", "status", "studio", "--json"], &[])
        .await;
    assert_eq!(state[0]["status"], "running", "{state}");
    let status = env
        .ok_json(
            &[
                "--machine",
                "studio",
                "--project",
                project_s,
                "status",
                "--json",
            ],
            &[],
        )
        .await;
    assert!(status.is_object(), "{status}");
    let agents = env
        .ok_json(
            &[
                "--machine",
                "Studio",
                "agents",
                "list",
                "--global",
                "--json",
            ],
            &[],
        )
        .await;
    assert_eq!(agents["summary"]["projects"], 1, "{agents}");

    let log = env.ssh_log();
    assert!(
        log.contains(
            "-T -o BatchMode=yes -o ControlMaster=no -o ControlPath=none fakehost uname -s"
        ),
        "{log}"
    );
    assert!(
        log.contains(
            "-T -o BatchMode=yes -o ServerAliveInterval=15 -o ServerAliveCountMax=3 fakehost pu daemon proxy"
        ),
        "{log}"
    );
    assert!(!log.contains("-L"), "a forward was opened: {log}");
    assert!(!log.contains("printf"), "{log}");
    // No forward socket, pid file or lock is ever created.
    assert!(
        env.machine_dir_entries(&id).is_empty(),
        "{:?}",
        env.machine_dir_entries(&id)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_unix_machine_should_relay_through_the_wrapped_proxy_command() {
    let env = Env::new();
    let _server = serve_remote_daemon(&env.remote_socket);
    env.add_studio().await;
    let unix = [("FAKE_REMOTE_OS", "unix")];
    let state = env
        .ok_json(&["machine", "status", "studio", "--json"], &unix)
        .await;
    assert_eq!(state[0]["status"], "running", "{state}");
    let health = env
        .ok_json(&["--machine", "studio", "health", "--json"], &unix)
        .await;
    assert!(health.is_object(), "{health}");
    let log = env.ssh_log();
    assert!(log.contains("exec pu daemon proxy"), "{log}");
    assert!(log.contains("/bin/sh -c"), "{log}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_machine_start_should_run_the_windows_or_unix_start_command() {
    let env = Env::new();
    env.add_studio().await;
    let output = env.pu(&["machine", "start", "studio"], &[]).await;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        env.ssh_log()
            .contains("-o ControlMaster=no -o ControlPath=none fakehost pu health"),
        "{}",
        env.ssh_log()
    );
    let output = env
        .pu(
            &["machine", "start", "studio"],
            &[("FAKE_REMOTE_OS", "unix")],
        )
        .await;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        env.ssh_log().contains("exec pu health"),
        "{}",
        env.ssh_log()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_unreachable_host_or_missing_pu_should_say_so() {
    let env = Env::new();
    env.add_studio().await;
    let state = env
        .ok_json(
            &["machine", "status", "studio", "--json"],
            &[("FAKE_SSH_FAIL", "1")],
        )
        .await;
    assert_eq!(state[0]["status"], "unreachable", "{state}");
    assert!(
        state[0]["detail"]
            .as_str()
            .unwrap()
            .contains("Could not resolve hostname"),
        "{state}"
    );
    let output = env
        .pu(
            &["--machine", "studio", "health"],
            &[("FAKE_SSH_FAIL", "1")],
        )
        .await;
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unreachable"));

    let state = env
        .ok_json(
            &["machine", "status", "studio", "--json"],
            &[("FAKE_NO_PU", "1")],
        )
        .await;
    assert_eq!(state[0]["status"], "unreachable", "{state}");
    assert!(
        state[0]["detail"]
            .as_str()
            .unwrap()
            .contains("PurePoint is not installed on machine studio"),
        "{state}"
    );
}

/// Real sshd on localhost, the current user's key authorized. The machine's
/// side is the ssh user's own `pu` and daemon (sshd builds that session's
/// environment, so the client's temp HOME cannot reach it): the test checks
/// the round trip, not which daemon state it finds.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_localhost_sshd_should_add_probe_and_target_the_machine() {
    let ssh = which::which("ssh").unwrap_or_else(|_| {
        PathBuf::from(std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into()))
            .join(r"System32\OpenSSH\ssh.exe")
    });
    let sshd_running = std::process::Command::new("sc.exe")
        .args(["query", "sshd"])
        .output()
        .map(|o| o.status.success() && String::from_utf8_lossy(&o.stdout).contains("RUNNING"))
        .unwrap_or(false);
    if !sshd_running {
        eprintln!("skipped: the OpenSSH Server service (sshd) is not installed or not running");
        return;
    }
    let login = std::process::Command::new(&ssh)
        .args([
            "-T",
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=10",
            "localhost",
            "pu --version",
        ])
        .stdin(std::process::Stdio::null())
        .output();
    match login {
        Ok(o) if o.status.success() => {}
        Ok(o) => {
            eprintln!(
                "skipped: `ssh localhost pu --version` failed ({}): {}",
                o.status,
                String::from_utf8_lossy(&o.stderr).trim()
            );
            return;
        }
        Err(e) => {
            eprintln!("skipped: cannot run {}: {e}", ssh.display());
            return;
        }
    }

    let tmp = tempfile::TempDir::new().unwrap();
    let home = tmp.path().join("h");
    std::fs::create_dir_all(&home).unwrap();
    let pu = |args: &[&str]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_pu"))
            .args(args)
            .env("HOME", &home)
            .env_remove("PU_SSH")
            .env_remove("PU_PROJECT_ROOT")
            .env_remove("PU_AGENT_ID")
            .output()
            .unwrap()
    };
    let added = pu(&["machine", "add", "local", "localhost", "--json"]);
    assert!(
        added.status.success(),
        "{}",
        String::from_utf8_lossy(&added.stderr)
    );
    let status = pu(&["machine", "status", "local", "--json"]);
    assert!(status.status.success());
    let state: serde_json::Value = serde_json::from_slice(&status.stdout).unwrap();
    let label = state[0]["status"].as_str().unwrap().to_string();
    assert!(
        label == "running" || label == "stopped",
        "localhost relay failed: {state}"
    );
    let targeted = pu(&["--machine", "local", "status", "--json"]);
    let stderr = String::from_utf8_lossy(&targeted.stderr);
    if label == "running" {
        // `status` needs a project on the machine; the relay itself worked.
        assert!(
            targeted.status.success() || stderr.contains("--project"),
            "{stderr}"
        );
    } else {
        assert!(stderr.contains("daemon not running on local"), "{stderr}");
    }
}

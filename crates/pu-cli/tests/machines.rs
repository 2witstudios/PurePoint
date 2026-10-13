//! `pu --machine` and `pu machine` against a fake ssh (`PU_SSH`). The fake
//! treats a local directory as the remote home and "forwards" by symlinking
//! the CLI's local socket to the daemon socket in that home.

use pu_core::protocol::{Request, Response};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

const FAKE_SSH: &str = r#"#!/bin/sh
echo "$*" >> "$FAKE_SSH_LOG"
if [ -n "$FAKE_SSH_FAIL" ]; then
  echo "ssh: Could not resolve hostname fakehost" >&2
  exit 255
fi
forward=""
prev=""
last=""
for arg in "$@"; do
  if [ "$prev" = "-L" ]; then forward="$arg"; fi
  prev="$arg"
  last="$arg"
done
if [ -n "$forward" ]; then
  if [ -n "$FAKE_SSH_NO_FORWARD" ]; then exec sleep 600; fi
  ln -s "${forward#*:}" "${forward%%:*}" || exit 255
  exec sleep 600
fi
case "$last" in
  *printf*) printf %s "$FAKE_REMOTE_HOME"; exit 0 ;;
  *"pu health"*) echo "remote daemon started"; exit 0 ;;
esac
echo "unexpected ssh invocation: $*" >&2
exit 255
"#;

/// Stands in for pu-engine on PATH: any local auto-start leaves a marker.
const FAKE_ENGINE: &str = "#!/bin/sh\ntouch \"$FAKE_ENGINE_MARKER\"\n";

struct Env {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    local_home: PathBuf,
    remote_socket: PathBuf,
    ssh: PathBuf,
    bin: PathBuf,
}

impl Env {
    fn new() -> Self {
        // Unix socket paths are limited to ~104 bytes; keep the root short.
        let tmp = tempfile::Builder::new()
            .prefix("pum")
            .tempdir_in("/tmp")
            .unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let local_home = root.join("h");
        let remote_home = root.join("r");
        std::fs::create_dir_all(&local_home).unwrap();
        std::fs::create_dir_all(remote_home.join(".pu")).unwrap();
        let bin = root.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let ssh = root.join("fake-ssh");
        for (path, body) in [(&ssh, FAKE_SSH), (&bin.join("pu-engine"), FAKE_ENGINE)] {
            std::fs::write(path, body).unwrap();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        Env {
            remote_socket: remote_home.join(".pu").join("daemon.sock"),
            _tmp: tmp,
            root,
            local_home,
            ssh,
            bin,
        }
    }

    fn engine_marker(&self) -> PathBuf {
        self.root.join("local-engine-started")
    }

    async fn pu(&self, args: &[&str], extra_env: &[(&str, &str)]) -> std::process::Output {
        self.command(args, extra_env).output().await.unwrap()
    }

    fn command(&self, args: &[&str], extra_env: &[(&str, &str)]) -> tokio::process::Command {
        let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_pu"));
        command
            .args(args)
            .current_dir(&self.root)
            .env("HOME", &self.local_home)
            .env("PATH", format!("{}:/usr/bin:/bin", self.bin.display()))
            .env("PU_SSH", &self.ssh)
            .env("FAKE_SSH_LOG", self.root.join("ssh.log"))
            .env("FAKE_REMOTE_HOME", self.root.join("r"))
            .env("FAKE_ENGINE_MARKER", self.engine_marker())
            .env_remove("PU_PROJECT_ROOT")
            .env_remove("PU_AGENT_ID")
            .env_remove("FAKE_SSH_FAIL")
            .env_remove("FAKE_SSH_NO_FORWARD");
        for (key, value) in extra_env {
            command.env(key, value);
        }
        command
    }

    async fn ok_json(&self, args: &[&str]) -> serde_json::Value {
        let output = self.pu(args, &[]).await;
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }

    async fn fails(&self, args: &[&str]) -> String {
        let output = self.pu(args, &[]).await;
        assert!(!output.status.success(), "{args:?} unexpectedly succeeded");
        String::from_utf8_lossy(&output.stderr).into_owned()
    }

    fn machine_dir(&self, machine_id: &str) -> PathBuf {
        self.local_home
            .join(".pu")
            .join("machines")
            .join(machine_id)
    }

    /// Registers `studio` -> `fakehost` and returns its id.
    async fn add_studio(&self) -> String {
        let added = self
            .ok_json(&["machine", "add", "studio", "fakehost", "--json"])
            .await;
        added["id"].as_str().unwrap().to_string()
    }

    fn ssh_log(&self) -> String {
        std::fs::read_to_string(self.root.join("ssh.log")).unwrap_or_default()
    }

    fn leftover_cli_sockets(&self, machine_id: &str) -> Vec<String> {
        std::fs::read_dir(self.machine_dir(machine_id))
            .map(|entries| {
                entries
                    .flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .filter(|n| n.starts_with("cli-"))
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// A real daemon on the "remote" socket; returns its task handle.
fn serve_remote_daemon(env: &Env) -> tokio::task::JoinHandle<()> {
    let registry = pu_core::paths::project_registry_path(&env.remote_socket);
    let engine = pu_engine::engine::Engine::with_project_registry(registry).unwrap();
    let server = pu_engine::ipc_server::IpcServer::bind(&env.remote_socket, engine).unwrap();
    tokio::spawn(async move {
        server.run().await.unwrap();
    })
}

/// A fake daemon on the "remote" socket: answers every request with `reply`,
/// or never answers when `reply` is `None`.
fn serve_fake_daemon(env: &Env, reply: Option<&'static str>) -> tokio::task::JoinHandle<()> {
    let listener = tokio::net::UnixListener::bind(&env.remote_socket).unwrap();
    tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((stream, _)) = listener.accept().await {
            match reply {
                Some(reply) => {
                    tokio::spawn(async move {
                        let (reader, mut writer) = stream.into_split();
                        let mut line = String::new();
                        let _ = BufReader::new(reader).read_line(&mut line).await;
                        let _ = writer.write_all(format!("{reply}\n").as_bytes()).await;
                    });
                }
                None => held.push(stream),
            }
        }
    })
}

/// Initializes a project on the remote daemon and returns its path.
async fn remote_project(env: &Env) -> String {
    let project = env.root.join("remote-project");
    std::fs::create_dir_all(&project).unwrap();
    let project_s = project.to_str().unwrap().to_string();
    env.ok_json(&[
        "--socket",
        env.remote_socket.to_str().unwrap(),
        "--project",
        &project_s,
        "init",
        "--json",
    ])
    .await;
    project_s
}

async fn wire(socket: &Path, request: Request) -> Response {
    let stream = tokio::net::UnixStream::connect(socket).await.unwrap();
    let (reader, mut writer) = stream.into_split();
    writer
        .write_all(format!("{}\n", serde_json::to_string(&request).unwrap()).as_bytes())
        .await
        .unwrap();
    let mut line = String::new();
    BufReader::new(reader).read_line(&mut line).await.unwrap();
    serde_json::from_str(&line).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_machine_registry_should_add_list_and_remove_machines() {
    let env = Env::new();
    env.add_studio().await;
    let registry_file = env.local_home.join(".pu").join("machines.json");
    assert_eq!(
        std::fs::metadata(&registry_file)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    let listed = env.ok_json(&["machine", "list", "--json"]).await;
    assert_eq!(listed[0]["name"], "studio");
    assert_eq!(listed[0]["ssh_host"], "fakehost");
    assert!(
        env.fails(&["machine", "add", "STUDIO", "other"])
            .await
            .contains("already exists")
    );
    env.ok_json(&["machine", "remove", "studio", "--json"])
        .await;
    let listed = env.ok_json(&["machine", "list", "--json"]).await;
    assert_eq!(listed, serde_json::json!([]));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_running_remote_daemon_should_serve_commands_over_a_private_forward() {
    let env = Env::new();
    let _server = serve_remote_daemon(&env);
    let id = env.add_studio().await;
    let project_s = remote_project(&env).await;

    // Inventory travels over the forward; names match case-insensitively.
    let projects = env
        .ok_json(&["--machine", "Studio", "projects", "list", "--json"])
        .await;
    assert_eq!(projects["projects"][0]["project_root"], project_s.as_str());
    let agents = env
        .ok_json(&[
            "--machine",
            "studio",
            "agents",
            "list",
            "--global",
            "--json",
        ])
        .await;
    assert_eq!(agents["summary"]["projects"], 1);
    let status = env
        .ok_json(&[
            "--machine",
            "studio",
            "--project",
            &project_s,
            "status",
            "--json",
        ])
        .await;
    assert!(status.is_object(), "{status}");
    let log = env.ssh_log();
    assert!(
        log.contains(
            "-o BatchMode=yes -o ControlMaster=no -o ControlPath=none fakehost printf %s \"$HOME\""
        ),
        "{log}"
    );
    assert!(
        log.contains("-o StreamLocalBindUnlink=yes -o ControlMaster=no -o ControlPath=none -L "),
        "{log}"
    );
    assert!(
        log.contains(&format!(
            ":{}/r/.pu/daemon.sock fakehost",
            env.root.display()
        )),
        "{log}"
    );
    let state = env
        .ok_json(&["machine", "status", "studio", "--json"])
        .await;
    assert_eq!(state[0]["status"], "running");
    // Every private forward was closed and its socket removed.
    assert!(env.leftover_cli_sockets(&id).is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_machine_target_should_refuse_local_only_and_unknown_targets() {
    let env = Env::new();
    let _server = serve_remote_daemon(&env);
    env.add_studio().await;
    // Local paths mean nothing on a machine.
    let error = env.fails(&["--machine", "studio", "status"]).await;
    assert!(error.contains("--project"), "{error}");
    let error = env
        .fails(&["--machine", "studio", "--project", "/srv/app", "init"])
        .await;
    assert!(error.contains("cannot be used with --machine"), "{error}");
    let error = env.fails(&["--machine", "nowhere", "health"]).await;
    assert!(error.contains("unknown machine"), "{error}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_live_app_forward_should_reuse_it_without_ssh() {
    let env = Env::new();
    let _server = serve_remote_daemon(&env);
    let id = env.add_studio().await;
    let project_s = remote_project(&env).await;
    let dir = env.machine_dir(&id);
    std::fs::create_dir_all(&dir).unwrap();
    // The app's forward: its local socket reaches the remote daemon.
    std::os::unix::fs::symlink(&env.remote_socket, dir.join("daemon.sock")).unwrap();

    let projects = env
        .ok_json(&["--machine", "studio", "projects", "list", "--json"])
        .await;
    assert_eq!(projects["projects"][0]["project_root"], project_s.as_str());
    assert_eq!(env.ssh_log(), "", "ssh ran despite a live app forward");
    assert!(env.leftover_cli_sockets(&id).is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_stopped_remote_daemon_should_fail_without_starting_a_local_one() {
    let env = Env::new();
    let server = serve_remote_daemon(&env);
    let id = env.add_studio().await;
    let project_s = remote_project(&env).await;
    assert!(matches!(
        wire(&env.remote_socket, Request::Shutdown).await,
        Response::ShuttingDown
    ));
    server.await.unwrap();

    let error = env
        .fails(&["--machine", "studio", "agents", "list", "--json"])
        .await;
    assert!(
        error.contains("daemon not running on studio") && error.contains("pu machine start studio"),
        "{error}"
    );
    let error = env
        .fails(&[
            "--machine",
            "studio",
            "--project",
            &project_s,
            "prompt",
            "list",
        ])
        .await;
    assert!(error.contains("pu machine start studio"), "{error}");
    assert!(!env.local_home.join(".pu").join("daemon.sock").exists());
    assert!(!env.engine_marker().exists(), "a local daemon was started");
    assert!(env.leftover_cli_sockets(&id).is_empty());
    let state = env.ok_json(&["machine", "status", "--json"]).await;
    assert_eq!(state[0]["status"], "stopped");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_machine_start_should_run_the_start_command_over_ssh() {
    let env = Env::new();
    env.add_studio().await;
    let output = env.pu(&["machine", "start", "studio"], &[]).await;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let log = env.ssh_log();
    assert!(
        log.contains(
            r#"-o ControlMaster=no -o ControlPath=none fakehost /bin/sh -c 'PATH="$HOME/.pu/bin:$HOME/.cargo/bin:$PATH"; export PATH; exec pu health'"#
        ),
        "{log}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_unreachable_host_should_report_unreachable() {
    let env = Env::new();
    env.add_studio().await;
    let output = env
        .pu(
            &["machine", "status", "studio", "--json"],
            &[("FAKE_SSH_FAIL", "1")],
        )
        .await;
    let state: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(state[0]["status"], "unreachable");
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
    assert!(!env.engine_marker().exists(), "a local daemon was started");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_busy_remote_daemon_should_report_busy() {
    let env = Env::new();
    let _daemon = serve_fake_daemon(
        &env,
        Some(r#"{"type":"error","code":"BUSY","message":"daemon connection limit reached"}"#),
    );
    let id = env.add_studio().await;
    let error = env
        .fails(&["--machine", "studio", "agents", "list", "--json"])
        .await;
    assert!(
        error.contains("daemon on studio") && error.contains("connection limit"),
        "{error}"
    );
    let state = env
        .ok_json(&["machine", "status", "studio", "--json"])
        .await;
    assert_eq!(state[0]["status"], "busy");
    assert!(!env.engine_marker().exists(), "a local daemon was started");
    assert!(env.leftover_cli_sockets(&id).is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_forward_never_ready_should_time_out_and_clean_up() {
    let env = Env::new();
    let id = env.add_studio().await;
    let output = env
        .pu(
            &["--machine", "studio", "health"],
            &[("FAKE_SSH_NO_FORWARD", "1")],
        )
        .await;
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("forward was not ready within"), "{error}");
    assert!(env.leftover_cli_sockets(&id).is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_socket_path_too_long_should_fail_before_running_ssh() {
    let env = Env::new();
    let home = env.root.join("h".repeat(60));
    std::fs::create_dir_all(&home).unwrap();
    let home_s = home.to_str().unwrap();
    let output = env
        .pu(
            &["machine", "add", "studio", "fakehost", "--json"],
            &[("HOME", home_s)],
        )
        .await;
    assert!(output.status.success());
    let output = env
        .pu(&["--machine", "studio", "health"], &[("HOME", home_s)])
        .await;
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("too long for a Unix socket"), "{error}");
    assert_eq!(env.ssh_log(), "", "ssh ran despite an unusable socket path");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_sigterm_should_close_the_private_forward() {
    let env = Env::new();
    // A daemon that accepts but never answers keeps the CLI waiting.
    let _daemon = serve_fake_daemon(&env, None);
    let id = env.add_studio().await;
    let mut child = env
        .command(&["--machine", "studio", "agents", "list", "--json"], &[])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let pid_file = env
        .machine_dir(&id)
        .join(format!("cli-{}.sock.pid", child.id().unwrap()));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let ssh_pid: libc::pid_t = loop {
        if let Some(pid) = std::fs::read_to_string(&pid_file)
            .ok()
            .and_then(|s| s.trim().parse().ok())
        {
            break pid;
        }
        assert!(std::time::Instant::now() < deadline, "forward never opened");
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    };
    // Let the CLI install its signal handler after the forward is up.
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    // SAFETY: plain signal delivery to the child this test spawned.
    unsafe {
        libc::kill(child.id().unwrap() as libc::pid_t, libc::SIGTERM);
    }
    let status = tokio::time::timeout(std::time::Duration::from_secs(5), child.wait())
        .await
        .expect("pu did not exit on SIGTERM")
        .unwrap();
    assert_eq!(status.code(), Some(128 + libc::SIGTERM), "{status}");
    assert!(env.leftover_cli_sockets(&id).is_empty());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    // SAFETY: signal 0 only checks for existence.
    while unsafe { libc::kill(ssh_pid, 0) } == 0 {
        assert!(
            std::time::Instant::now() < deadline,
            "forward ssh {ssh_pid} survived SIGTERM"
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
}

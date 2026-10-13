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
            .env_remove("FAKE_SSH_FAIL");
        for (key, value) in extra_env {
            command.env(key, value);
        }
        command.output().await.unwrap()
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

    fn leftover_cli_sockets(&self, machine_id: &str) -> Vec<String> {
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
                    .filter(|n| n.starts_with("cli-"))
                    .collect()
            })
            .unwrap_or_default()
    }
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
async fn given_registered_machine_should_reach_its_daemon_over_ssh_and_never_start_one() {
    let env = Env::new();
    let registry = pu_core::paths::project_registry_path(&env.remote_socket);
    let engine = pu_engine::engine::Engine::with_project_registry(registry).unwrap();
    let server = pu_engine::ipc_server::IpcServer::bind(&env.remote_socket, engine).unwrap();
    let handle = tokio::spawn(async move {
        server.run().await.unwrap();
    });

    // Register the machine.
    let added = env
        .ok_json(&["machine", "add", "studio", "fakehost", "--json"])
        .await;
    let id = added["id"].as_str().unwrap().to_string();
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

    // A project that exists only on the "remote" daemon.
    let project = env.root.join("remote-project");
    std::fs::create_dir_all(&project).unwrap();
    let project_s = project.to_str().unwrap();
    let socket_s = env.remote_socket.to_str().unwrap();
    env.ok_json(&[
        "--socket",
        socket_s,
        "--project",
        project_s,
        "init",
        "--json",
    ])
    .await;

    // Inventory travels over the forward; names match case-insensitively.
    let projects = env
        .ok_json(&["--machine", "Studio", "projects", "list", "--json"])
        .await;
    assert_eq!(projects["projects"][0]["project_root"], project_s);
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
            project_s,
            "status",
            "--json",
        ])
        .await;
    assert!(status.is_object(), "{status}");
    let log = std::fs::read_to_string(env.root.join("ssh.log")).unwrap();
    assert!(
        log.contains("-o BatchMode=yes fakehost printf %s \"$HOME\""),
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

    // Local paths mean nothing on a machine.
    let error = env.fails(&["--machine", "studio", "status"]).await;
    assert!(error.contains("--project"), "{error}");
    let error = env
        .fails(&["--machine", "studio", "--project", project_s, "init"])
        .await;
    assert!(error.contains("cannot be used with --machine"), "{error}");
    let error = env.fails(&["--machine", "nowhere", "health"]).await;
    assert!(error.contains("unknown machine"), "{error}");

    let state = env
        .ok_json(&["machine", "status", "studio", "--json"])
        .await;
    assert_eq!(state[0]["status"], "running");
    // Every private forward was closed and its socket removed.
    assert!(env.leftover_cli_sockets(&id).is_empty());

    // Stop the remote daemon: commands fail without starting a local one.
    assert!(matches!(
        wire(&env.remote_socket, Request::Shutdown).await,
        Response::ShuttingDown
    ));
    handle.await.unwrap();
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
            project_s,
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
    let output = env.pu(&["machine", "start", "studio"], &[]).await;
    assert!(output.status.success());
    let log = std::fs::read_to_string(env.root.join("ssh.log")).unwrap();
    assert!(
        log.contains(
            r#"fakehost PATH="$HOME/.pu/bin:$HOME/.cargo/bin:$PATH"; export PATH; pu health"#
        ),
        "{log}"
    );

    // Unreachable host.
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

    env.ok_json(&["machine", "remove", "studio", "--json"])
        .await;
    let listed = env.ok_json(&["machine", "list", "--json"]).await;
    assert_eq!(listed, serde_json::json!([]));
}

//! `pu daemon proxy` as a real process, the way an SSH exec channel runs it.

use pu_core::protocol::{PROTOCOL_VERSION, Request, Response};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

/// Stands in for pu-engine on PATH: any auto-start leaves a marker.
const FAKE_ENGINE: &str = "#!/bin/sh\ntouch \"$FAKE_ENGINE_MARKER\"\n";

struct Env {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    socket: PathBuf,
}

impl Env {
    fn new() -> Self {
        // Unix socket paths are limited to ~104 bytes; keep the root short.
        let tmp = tempfile::Builder::new()
            .prefix("pup")
            .tempdir_in("/tmp")
            .unwrap();
        let root = tmp.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join("h").join(".pu")).unwrap();
        std::fs::create_dir_all(root.join("bin")).unwrap();
        let engine = root.join("bin").join("pu-engine");
        std::fs::write(&engine, FAKE_ENGINE).unwrap();
        std::fs::set_permissions(&engine, std::fs::Permissions::from_mode(0o755)).unwrap();
        Env {
            socket: root.join("h").join(".pu").join("daemon.sock"),
            _tmp: tmp,
            root,
        }
    }

    fn engine_marker(&self) -> PathBuf {
        self.root.join("engine-started")
    }

    fn proxy(&self, args: &[&str]) -> tokio::process::Command {
        let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_pu"));
        command
            .args(args)
            .args(["daemon", "proxy"])
            .current_dir(&self.root)
            .env("HOME", self.root.join("h"))
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", self.root.join("bin").display()),
            )
            .env("FAKE_ENGINE_MARKER", self.engine_marker())
            .env_remove("PU_PROJECT_ROOT")
            .env_remove("PU_AGENT_ID")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }
}

fn health_report() -> Response {
    Response::HealthReport {
        pid: 4242,
        uptime_seconds: 7,
        protocol_version: PROTOCOL_VERSION,
        projects: vec!["/remote/project".into()],
        agent_count: 3,
    }
}

/// A fake daemon speaking the real protocol: one JSON request line in, one
/// JSON response line out per line, until the client half-closes.
fn serve_fake_daemon(socket: &std::path::Path) -> tokio::task::JoinHandle<Vec<String>> {
    let listener = tokio::net::UnixListener::bind(socket).unwrap();
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let (reader, mut writer) = stream.into_split();
        let mut lines = BufReader::new(reader).lines();
        let mut seen = Vec::new();
        while let Some(line) = lines.next_line().await.unwrap() {
            let response = match serde_json::from_str::<Request>(&line) {
                Ok(Request::Health) => health_report(),
                _ => Response::Error {
                    code: "UNEXPECTED".into(),
                    message: line.clone(),
                },
            };
            let mut out = serde_json::to_string(&response).unwrap();
            out.push('\n');
            writer.write_all(out.as_bytes()).await.unwrap();
            seen.push(line);
        }
        seen
    })
}

#[tokio::test]
async fn given_health_line_should_emit_daemon_health_response_line() {
    // given
    let env = Env::new();
    let daemon = serve_fake_daemon(&env.socket);
    let mut child = env
        .proxy(&["--socket", env.socket.to_str().unwrap()])
        .spawn()
        .unwrap();

    // when
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(b"{\"type\":\"health\"}\n").await.unwrap();
    drop(stdin);
    let output = tokio::time::timeout(std::time::Duration::from_secs(10), child.wait_with_output())
        .await
        .expect("proxy did not exit")
        .unwrap();

    // then
    let mut expected = serde_json::to_string(&health_report()).unwrap();
    expected.push('\n');
    assert_eq!(String::from_utf8(output.stdout).unwrap(), expected);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stderr, b"");
    assert_eq!(
        daemon.await.unwrap(),
        vec!["{\"type\":\"health\"}".to_string()]
    );
}

#[tokio::test]
async fn given_default_socket_should_relay_to_home_daemon() {
    // given: no --socket, so the daemon is found at ~/.pu/daemon.sock
    let env = Env::new();
    let daemon = serve_fake_daemon(&env.socket);
    let mut child = env.proxy(&[]).spawn().unwrap();

    // when
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(b"{\"type\":\"health\"}\n").await.unwrap();
    drop(stdin);
    let mut stdout = String::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut stdout)
        .await
        .unwrap();
    let status = child.wait().await.unwrap();

    // then
    let reply: Response = serde_json::from_str(stdout.trim_end()).unwrap();
    assert!(matches!(reply, Response::HealthReport { pid: 4242, .. }));
    assert!(status.success());
    daemon.await.unwrap();
}

#[tokio::test]
async fn given_no_daemon_should_exit_nonzero_with_empty_stdout_and_not_start_one() {
    // given
    let env = Env::new();

    // when
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        env.proxy(&["--socket", env.socket.to_str().unwrap()])
            .output(),
    )
    .await
    .expect("proxy did not exit")
    .unwrap();

    // then
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert_eq!(output.stdout, b"");
    assert!(stderr.contains(env.socket.to_str().unwrap()), "{stderr}");
    assert!(stderr.contains("pu daemon start"), "{stderr}");
    assert!(!env.engine_marker().exists(), "proxy started a daemon");
    assert!(!env.socket.exists());
}

#[tokio::test]
async fn given_real_daemon_should_relay_its_health_report() {
    // given
    let env = Env::new();
    let registry = pu_core::paths::project_registry_path(&env.socket);
    let engine = pu_engine::engine::Engine::with_project_registry(registry).unwrap();
    let server = pu_engine::ipc_server::IpcServer::bind(&env.socket, engine).unwrap();
    tokio::spawn(async move {
        server.run().await.unwrap();
    });
    let mut child = env
        .proxy(&["--socket", env.socket.to_str().unwrap()])
        .spawn()
        .unwrap();

    // when
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(b"{\"type\":\"health\"}\n").await.unwrap();
    drop(stdin);
    let output = tokio::time::timeout(std::time::Duration::from_secs(10), child.wait_with_output())
        .await
        .expect("proxy did not exit")
        .unwrap();

    // then
    let stdout = String::from_utf8(output.stdout).unwrap();
    let reply: Response = serde_json::from_str(stdout.trim_end()).unwrap();
    assert!(
        matches!(reply, Response::HealthReport { protocol_version, .. } if protocol_version == PROTOCOL_VERSION),
        "{stdout}"
    );
    assert_eq!(stdout.matches('\n').count(), 1);
    assert!(output.status.success());
    assert_eq!(output.stderr, b"");
}

//! The real `pu-engine` binary: single instance, health over the platform
//! transport, and `--parent-pid`. Runs on every platform.

use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

use pu_core::protocol::{Request, Response};
use pu_transport::Endpoint;
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

fn engine(home: &Path, socket: &Path) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_pu-engine"));
    command
        .arg("--socket")
        .arg(socket)
        .env("HOME", home)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    command
}

async fn send(socket: &Path, request: &Request) -> std::io::Result<Response> {
    let stream = pu_transport::connect(&Endpoint::for_socket(socket)?).await?;
    let (reader, mut writer) = stream.into_split();
    let line = serde_json::to_string(request)? + "\n";
    writer.write_all(line.as_bytes()).await?;
    let mut reply = String::new();
    BufReader::new(reader).read_line(&mut reply).await?;
    Ok(serde_json::from_str(&reply)?)
}

async fn wait_healthy(socket: &Path, process: &mut tokio::process::Child) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            assert!(
                process.try_wait().unwrap().is_none(),
                "daemon exited before serving"
            );
            if let Ok(Response::HealthReport { .. }) = send(socket, &Request::Health).await {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("daemon never answered health");
}

/// A long-lived child to stand in for the app that launched the daemon.
fn sleeper() -> std::process::Child {
    #[cfg(windows)]
    let mut command = {
        let mut c = std::process::Command::new("ping");
        c.args(["-n", "60", "127.0.0.1"]);
        c
    };
    #[cfg(unix)]
    let mut command = {
        let mut c = std::process::Command::new("sleep");
        c.arg("60");
        c
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_running_daemon_second_instance_should_exit_with_already_running() {
    let home = TempDir::new().unwrap();
    let socket = home.path().join(".pu").join("daemon.sock");
    let mut first = engine(home.path(), &socket).spawn().unwrap();
    wait_healthy(&socket, &mut first).await;

    let second = engine(home.path(), &socket).output().await.unwrap();
    let stderr = String::from_utf8_lossy(&second.stderr);
    assert!(stderr.contains("already running"), "{stderr}");
    // The first daemon keeps its endpoint.
    assert!(matches!(
        send(&socket, &Request::Health).await.unwrap(),
        Response::HealthReport { .. }
    ));

    send(&socket, &Request::Shutdown).await.unwrap();
    let status = tokio::time::timeout(Duration::from_secs(10), first.wait())
        .await
        .unwrap()
        .unwrap();
    assert!(status.success());
    assert!(!socket.with_extension("sock.pid").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_parent_pid_should_exit_within_two_seconds_of_parent_death() {
    let home = TempDir::new().unwrap();
    let socket = home.path().join(".pu").join("daemon.sock");
    let mut parent = sleeper();
    let mut daemon = engine(home.path(), &socket)
        .arg("--managed")
        .arg("--parent-pid")
        .arg(parent.id().to_string())
        .spawn()
        .unwrap();
    wait_healthy(&socket, &mut daemon).await;

    parent.kill().unwrap();
    parent.wait().unwrap();
    let died = Instant::now();
    // Unix polls every 2 s; Windows waits on the handle.
    let budget = if cfg!(windows) {
        Duration::from_secs(2)
    } else {
        Duration::from_secs(5)
    };
    let status = tokio::time::timeout(budget, daemon.wait())
        .await
        .unwrap_or_else(|_| {
            panic!(
                "daemon still running {:?} after parent died",
                died.elapsed()
            )
        })
        .unwrap();
    assert!(status.success());
}

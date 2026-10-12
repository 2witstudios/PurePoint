//! Real isolated daemon startup/serving proofs. Never uses the installed daemon.
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use pu_core::protocol::{Request, Response};
use pu_engine::daemon_lifecycle::{lock_path_for, pid_path_for, try_acquire_daemon_lock};
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::process::{Child, Command};
use tokio::time::{sleep, timeout};

fn private_dir() -> TempDir {
    tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap()
}

fn launch(socket: &Path, managed: bool) -> Child {
    let mut command = Command::new(env!("CARGO_BIN_EXE_pu-engine"));
    command
        .arg("--socket")
        .arg(socket)
        .kill_on_drop(true)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    if managed {
        command.arg("--managed");
    }
    command.spawn().unwrap()
}

async fn request(socket: &Path, request: &Request) -> std::io::Result<Response> {
    timeout(Duration::from_secs(2), async {
        let stream = UnixStream::connect(socket).await?;
        let (reader, mut writer) = stream.into_split();
        let mut json = serde_json::to_vec(request)?;
        json.push(b'\n');
        writer.write_all(&json).await?;
        let mut line = String::new();
        BufReader::new(reader).read_line(&mut line).await?;
        Ok(serde_json::from_str(&line)?)
    })
    .await
    .expect("bounded IPC request")
}

async fn ready(socket: &Path, child: &mut Child) -> u32 {
    timeout(Duration::from_secs(5), async {
        loop {
            assert!(
                child.try_wait().unwrap().is_none(),
                "daemon exited before readiness"
            );
            if let Ok(Response::HealthReport { pid, .. }) = request(socket, &Request::Health).await
            {
                return pid;
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("daemon should become ready")
}

#[tokio::test]
async fn given_both_startup_modes_should_lock_serve_owner_and_cleanup_on_shutdown() {
    for managed in [false, true] {
        let tmp = private_dir();
        let socket = tmp.path().join("daemon.sock");
        drop(std::os::unix::net::UnixListener::bind(&socket).unwrap());
        let mut first = launch(&socket, managed);
        assert_eq!(ready(&socket, &mut first).await, first.id().unwrap());
        let identity = fs::metadata(&socket).unwrap().ino();
        assert_eq!(fs::metadata(&socket).unwrap().mode() & 0o777, 0o600);
        assert_eq!(
            fs::read_to_string(pid_path_for(&socket)).unwrap().trim(),
            first.id().unwrap().to_string()
        );

        let second = launch(&socket, !managed);
        let output = timeout(Duration::from_secs(5), second.wait_with_output())
            .await
            .unwrap()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(fs::metadata(&socket).unwrap().ino(), identity);
        assert!(matches!(
            request(&socket, &Request::Health).await.unwrap(),
            Response::HealthReport { .. }
        ));
        assert!(matches!(
            request(&socket, &Request::Shutdown).await.unwrap(),
            Response::ShuttingDown
        ));
        assert!(
            timeout(Duration::from_secs(5), first.wait())
                .await
                .unwrap()
                .unwrap()
                .success()
        );
        assert!(!socket.exists());
        assert!(!pid_path_for(&socket).exists());
        assert!(lock_path_for(&socket).exists());
        assert!(try_acquire_daemon_lock(&socket).unwrap().is_some());
    }
}

#[tokio::test]
async fn given_startup_failure_should_cleanup_claimed_pid_and_preserve_unrelated_entries() {
    for failure in ["file", "symlink", "registry"] {
        let tmp = private_dir();
        let socket = tmp.path().join("daemon.sock");
        let sentinel = tmp.path().join("unrelated");
        fs::write(&sentinel, "untouched").unwrap();
        match failure {
            "file" => fs::write(&socket, "untouched").unwrap(),
            "symlink" => symlink(&sentinel, &socket).unwrap(),
            _ => fs::write(
                pu_core::paths::project_registry_path(&socket),
                "invalid JSON",
            )
            .unwrap(),
        }
        let before = fs::symlink_metadata(&socket).ok();
        let child = launch(&socket, false);
        let output = timeout(Duration::from_secs(5), child.wait_with_output())
            .await
            .unwrap()
            .unwrap();
        assert!(!output.status.success(), "{failure}");
        assert!(!pid_path_for(&socket).exists(), "{failure}");
        assert_eq!(fs::read_to_string(&sentinel).unwrap(), "untouched");
        if let Some(before) = before {
            let after = fs::symlink_metadata(&socket).unwrap();
            assert_eq!((after.ino(), after.mode()), (before.ino(), before.mode()));
        } else {
            assert!(!socket.exists());
        }
        assert!(try_acquire_daemon_lock(&socket).unwrap().is_some());
    }
}

#[tokio::test]
async fn given_shared_directory_should_fail_startup_without_changing_permissions_or_creating_metadata()
 {
    let tmp = private_dir();
    fs::set_permissions(tmp.path(), fs::Permissions::from_mode(0o755)).unwrap();
    let socket = tmp.path().join("daemon.sock");
    let output = timeout(
        Duration::from_secs(5),
        launch(&socket, true).wait_with_output(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("0700"));
    assert_eq!(fs::metadata(tmp.path()).unwrap().mode() & 0o777, 0o755);
    assert!(!socket.exists());
    assert!(!lock_path_for(&socket).exists());
    assert!(!pid_path_for(&socket).exists());
}

#[tokio::test]
#[ignore = "requires root to launch an actual different-UID client"]
async fn given_foreign_peer_should_not_dispatch_even_when_socket_permissions_are_relaxed() {
    // Re-enter just this test as an unprivileged client, using the real socket
    // and credential lookup rather than a synthetic credential structure.
    const CLIENT_SOCKET: &str = "PU_SECURITY_TEST_FOREIGN_SOCKET";
    if let Some(socket) = std::env::var_os(CLIENT_SOCKET) {
        assert_ne!(unsafe { libc::geteuid() }, 0);
        assert!(
            request(Path::new(&socket), &Request::Shutdown)
                .await
                .is_err()
        );
        return;
    }
    assert_eq!(unsafe { libc::geteuid() }, 0);
    let tmp = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in("/tmp")
        .unwrap();
    let socket = tmp.path().join("daemon.sock");
    let mut daemon = launch(&socket, false);
    ready(&socket, &mut daemon).await;
    fs::set_permissions(tmp.path(), fs::Permissions::from_mode(0o755)).unwrap();
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o666)).unwrap();
    let mut client = Command::new(std::env::current_exe().unwrap());
    client
        .arg("--exact")
        .arg("given_foreign_peer_should_not_dispatch_even_when_socket_permissions_are_relaxed")
        .arg("--ignored")
        .env(CLIENT_SOCKET, &socket)
        .current_dir("/tmp")
        .uid(65534)
        .gid(65534)
        .kill_on_drop(true);
    let output = timeout(Duration::from_secs(5), client.output())
        .await
        .unwrap()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(daemon.try_wait().unwrap().is_none());
    fs::set_permissions(tmp.path(), fs::Permissions::from_mode(0o700)).unwrap();
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(matches!(
        request(&socket, &Request::Shutdown).await.unwrap(),
        Response::ShuttingDown
    ));
    assert!(
        timeout(Duration::from_secs(5), daemon.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
}

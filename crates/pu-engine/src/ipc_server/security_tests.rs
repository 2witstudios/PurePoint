use super::*;
use std::fs;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt, symlink};
use std::os::unix::net::UnixListener as StdListener;
use tempfile::TempDir;

fn private_dir() -> TempDir {
    tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap()
}

#[tokio::test]
async fn given_non_socket_entries_should_refuse_binding_without_deleting_them() {
    for kind in ["file", "directory", "symlink", "dangling_symlink"] {
        let tmp = private_dir();
        let socket = tmp.path().join("daemon.sock");
        let target = tmp.path().join("target");
        fs::write(&target, b"preserve me").unwrap();
        match kind {
            "file" => fs::write(&socket, b"preserve me").unwrap(),
            "directory" => fs::create_dir(&socket).unwrap(),
            "symlink" => symlink(&target, &socket).unwrap(),
            _ => symlink(tmp.path().join("absent"), &socket).unwrap(),
        }
        let before = fs::symlink_metadata(&socket).unwrap();

        assert!(IpcServer::bind(&socket, Engine::new()).is_err(), "{kind}");

        let after = fs::symlink_metadata(&socket).unwrap();
        assert_eq!(after.ino(), before.ino(), "{kind}");
        assert_eq!(after.mode(), before.mode(), "{kind}");
        assert_eq!(fs::read(&target).unwrap(), b"preserve me");
    }
}

#[tokio::test]
async fn given_shared_socket_directory_should_refuse_without_chmod_or_unlink() {
    for mode in [0o755, 0o750, 0o770, 0o777, 0o1777] {
        let tmp = private_dir();
        fs::set_permissions(tmp.path(), fs::Permissions::from_mode(mode)).unwrap();
        let socket = tmp.path().join("daemon.sock");
        let stale = StdListener::bind(&socket).unwrap();
        drop(stale);
        let before = fs::symlink_metadata(&socket).unwrap();

        assert!(IpcServer::bind(&socket, Engine::new()).is_err(), "{mode:o}");

        assert_eq!(fs::metadata(tmp.path()).unwrap().mode() & 0o7777, mode);
        assert_eq!(fs::symlink_metadata(&socket).unwrap().ino(), before.ino());
    }
}

#[tokio::test]
async fn given_symlink_socket_parent_should_refuse_without_touching_target() {
    let tmp = private_dir();
    let target = private_dir();
    let alias = tmp.path().join("alias");
    symlink(target.path(), &alias).unwrap();
    let socket = target.path().join("daemon.sock");
    let stale = StdListener::bind(&socket).unwrap();
    drop(stale);
    let before = fs::metadata(&socket).unwrap();

    assert!(IpcServer::bind(&alias.join("daemon.sock"), Engine::new()).is_err());
    assert_eq!(fs::metadata(&socket).unwrap().ino(), before.ino());
}

#[tokio::test]
async fn given_live_unlocked_socket_should_refuse_to_replace_it() {
    let tmp = private_dir();
    let socket = tmp.path().join("daemon.sock");
    let listener = StdListener::bind(&socket).unwrap();
    let before = fs::metadata(&socket).unwrap();

    assert!(IpcServer::bind(&socket, Engine::new()).is_err());
    assert_eq!(fs::metadata(&socket).unwrap().ino(), before.ino());
    assert!(std::os::unix::net::UnixStream::connect(&socket).is_ok());
    drop(listener);
}

#[tokio::test]
async fn given_stale_owned_socket_should_bind_privately_and_hold_lock_until_drop() {
    let tmp = private_dir();
    let socket = tmp.path().join("daemon.sock");
    drop(StdListener::bind(&socket).unwrap());

    let server = IpcServer::bind(&socket, Engine::new()).unwrap();

    let metadata = fs::symlink_metadata(&socket).unwrap();
    assert!(metadata.file_type().is_socket());
    assert_eq!(metadata.mode() & 0o777, 0o600);
    assert_eq!(metadata.uid(), unsafe { libc::geteuid() });
    assert!(
        crate::daemon_lifecycle::try_acquire_daemon_lock(&socket)
            .unwrap()
            .is_none()
    );
    assert!(IpcServer::bind(&socket, Engine::new()).is_err());
    drop(server);
    assert!(!socket.exists());
    assert!(
        crate::daemon_lifecycle::try_acquire_daemon_lock(&socket)
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn given_replaced_socket_should_preserve_replacement_during_cleanup() {
    let tmp = private_dir();
    let socket = tmp.path().join("daemon.sock");
    let server = IpcServer::bind(&socket, Engine::new()).unwrap();
    fs::remove_file(&socket).unwrap();
    fs::write(&socket, b"replacement").unwrap();

    drop(server);

    assert_eq!(fs::read(&socket).unwrap(), b"replacement");
}

#[tokio::test]
async fn given_kernel_peer_credentials_should_accept_only_the_expected_owner() {
    let tmp = private_dir();
    let socket = tmp.path().join("daemon.sock");
    let server = IpcServer::bind(&socket, Engine::new()).unwrap();
    let _client = tokio::net::UnixStream::connect(&socket).await.unwrap();
    let (accepted, _) = server.listener.accept().await.unwrap();
    // Exercise the real OS credential query with both acceptance policies.
    let owner = unsafe { libc::geteuid() };
    assert_eq!(accepted.peer_cred().unwrap().uid(), owner);
    validate_peer(&accepted, owner).unwrap();
    assert_eq!(
        validate_peer(&accepted, owner.wrapping_add(1))
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::PermissionDenied
    );
}

#[tokio::test]
async fn given_shutdown_drain_should_retain_lock_until_lifecycle_guard_is_released() {
    let tmp = private_dir();
    let socket = tmp.path().join("daemon.sock");
    let server = IpcServer::bind(&socket, Engine::new()).unwrap();
    let lifecycle = server.lifecycle();
    drop(server);
    assert!(
        crate::daemon_lifecycle::try_acquire_daemon_lock(&socket)
            .unwrap()
            .is_none()
    );
    lifecycle.cleanup();
    assert!(!socket.exists());
    assert!(
        crate::daemon_lifecycle::try_acquire_daemon_lock(&socket)
            .unwrap()
            .is_none()
    );
    drop(lifecycle);
    assert!(
        crate::daemon_lifecycle::try_acquire_daemon_lock(&socket)
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn given_shutdown_or_abort_should_close_old_clients_before_successor_serves() {
    for abort in [false, true] {
        let tmp = private_dir();
        let socket = tmp.path().join("daemon.sock");
        let server = IpcServer::bind(&socket, Engine::new()).unwrap();
        let shutdown = server.shutdown.clone();
        let task = tokio::spawn(server.run());
        let stream = tokio::net::UnixStream::connect(&socket).await.unwrap();
        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);
        writer.write_all(b"{\"type\":\"health\"}\n").await.unwrap();
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        assert!(matches!(
            serde_json::from_str::<Response>(&line).unwrap(),
            Response::HealthReport { .. }
        ));

        if abort {
            task.abort();
            assert!(task.await.unwrap_err().is_cancelled());
        } else {
            shutdown.notify_one();
            task.await.unwrap().unwrap();
        }
        line.clear();
        assert_eq!(
            tokio::time::timeout(
                std::time::Duration::from_secs(1),
                reader.read_line(&mut line)
            )
            .await
            .expect("shutdown must close accepted clients")
            .unwrap(),
            0
        );

        let successor = IpcServer::bind(&socket, Engine::new()).unwrap();
        let successor_task = tokio::spawn(successor.run());
        // The old descriptor cannot dispatch another request into either engine.
        let _ = writer.write_all(b"{\"type\":\"shutdown\"}\n").await;
        assert_eq!(reader.read_line(&mut line).await.unwrap(), 0);
        let mut client = tokio::net::UnixStream::connect(&socket).await.unwrap();
        client.write_all(b"{\"type\":\"health\"}\n").await.unwrap();
        let mut client = BufReader::new(client);
        client.read_line(&mut line).await.unwrap();
        assert!(matches!(
            serde_json::from_str::<Response>(&line).unwrap(),
            Response::HealthReport { .. }
        ));
        assert!(!successor_task.is_finished());
        client
            .get_mut()
            .write_all(b"{\"type\":\"shutdown\"}\n")
            .await
            .unwrap();
        line.clear();
        client.read_line(&mut line).await.unwrap();
        assert!(matches!(
            serde_json::from_str::<Response>(&line).unwrap(),
            Response::ShuttingDown
        ));
        successor_task.await.unwrap().unwrap();
    }
}

#[tokio::test]
async fn given_replacement_socket_or_symlink_should_not_unlink_it_on_drop() {
    for link in [false, true] {
        let tmp = private_dir();
        let socket = tmp.path().join("daemon.sock");
        let target = tmp.path().join("target");
        fs::write(&target, "untouched").unwrap();
        let server = IpcServer::bind(&socket, Engine::new()).unwrap();
        fs::remove_file(&socket).unwrap();
        let replacement = if link {
            symlink(&target, &socket).unwrap();
            None
        } else {
            Some(StdListener::bind(&socket).unwrap())
        };
        let before = fs::symlink_metadata(&socket).unwrap().ino();
        drop(server);
        assert_eq!(fs::symlink_metadata(&socket).unwrap().ino(), before);
        assert_eq!(fs::read_to_string(&target).unwrap(), "untouched");
        drop(replacement);
    }
}

#[tokio::test]
#[ignore = "requires root to create actual foreign-owned filesystem fixtures"]
async fn given_foreign_owned_entries_should_reject_without_chmod_or_unlink() {
    use std::os::unix::ffi::OsStrExt;
    assert_eq!(unsafe { libc::geteuid() }, 0);
    for kind in ["directory", "socket", "lock", "pid"] {
        let tmp = tempfile::Builder::new()
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir_in("/tmp")
            .unwrap();
        let socket = tmp.path().join("daemon.sock");
        let path = match kind {
            "directory" => tmp.path().to_owned(),
            "socket" => {
                drop(StdListener::bind(&socket).unwrap());
                socket.clone()
            }
            "lock" => crate::daemon_lifecycle::lock_path_for(&socket),
            _ => crate::daemon_lifecycle::pid_path_for(&socket),
        };
        if matches!(kind, "lock" | "pid") {
            fs::write(&path, "12345\n").unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        }
        let cpath = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        // SAFETY: this changes only a fixture created in this test's tempdir.
        assert_eq!(unsafe { libc::chown(cpath.as_ptr(), 65534, !0) }, 0);
        let before = fs::symlink_metadata(&path).unwrap();
        if kind == "pid" {
            let mut lock = crate::daemon_lifecycle::try_acquire_daemon_lock(&socket)
                .unwrap()
                .unwrap();
            assert!(lock.claim_pid_file().is_err());
        } else {
            assert!(IpcServer::bind(&socket, Engine::new()).is_err(), "{kind}");
        }
        let after = fs::symlink_metadata(&path).unwrap();
        assert_eq!(
            (after.ino(), after.uid(), after.mode()),
            (before.ino(), before.uid(), before.mode())
        );
    }
}

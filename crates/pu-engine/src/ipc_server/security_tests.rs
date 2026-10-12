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

// Observe the real manifest updater's descriptor before cancelling transport.
// A second open descriptor for this unique locked inode proves the blocking
// closure has entered update_manifest, without a production marker or mock.
pub(crate) fn another_descriptor_for(file: &std::fs::File) -> bool {
    use std::os::fd::AsRawFd;
    let metadata = file.metadata().unwrap();
    // SAFETY: sysconf has no memory arguments; fstat receives valid storage and
    // safely reports EBADF for closed descriptor numbers.
    let limit = unsafe { libc::sysconf(libc::_SC_OPEN_MAX) }.clamp(0, 65_536);
    for fd in 0..limit as i32 {
        if fd == file.as_raw_fd() {
            continue;
        }
        let mut stat: libc::stat = unsafe { std::mem::zeroed() };
        if unsafe { libc::fstat(fd, &mut stat) } != 0 {
            continue;
        }
        // st_dev is i32 on macOS and u64 on Linux.
        #[allow(clippy::unnecessary_cast)]
        let device = stat.st_dev as u64;
        if device == metadata.dev() && stat.st_ino == metadata.ino() {
            return true;
        }
    }
    false
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_blocked_rename_should_keep_singleton_until_old_effect_finishes_after_shutdown_or_abort()
 {
    use nix::fcntl::{Flock, FlockArg};
    use pu_core::{
        manifest, paths,
        types::{AgentEntry, Manifest},
    };
    use std::time::Duration;
    for stopping in ["graceful", "abort", "timeout"] {
        let abort = stopping == "abort";
        let tmp = private_dir();
        let root = tmp.path().join("project");
        fs::create_dir_all(paths::pu_dir(&root)).unwrap();
        let mut manifest = Manifest::new(root.to_string_lossy().into_owned());
        let agent: AgentEntry = serde_json::from_value(serde_json::json!({
            "id":"ag-effect", "name":"before", "agentType":"claude",
            "status":"running", "startedAt":"2026-01-01T00:00:00Z"
        }))
        .unwrap();
        manifest.agents.insert(agent.id.clone(), agent);
        manifest::write_manifest(&root, &manifest).unwrap();
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(paths::manifest_path(&root).with_extension("json.lock"))
            .unwrap();
        let manifest_lock = Flock::lock(file, FlockArg::LockExclusive).unwrap();
        let socket = tmp.path().join("daemon.sock");
        let mut lock = crate::daemon_lifecycle::try_acquire_daemon_lock(&socket)
            .unwrap()
            .unwrap();
        lock.claim_pid_file().unwrap();
        let server = IpcServer::bind_with_lock(lock, Engine::new()).unwrap();
        let shutdown = server.shutdown.clone();
        let work = server.work();
        let lifecycle = server.lifecycle();
        let task = tokio::spawn(server.run());
        let mut task = Some(task);
        let mut client = tokio::net::UnixStream::connect(&socket).await.unwrap();
        let rename = Request::Rename {
            project_root: root.to_string_lossy().into_owned(),
            agent_id: "ag-effect".into(),
            name: "old-effect-completed".into(),
        };
        client
            .write_all((serde_json::to_string(&rename).unwrap() + "\n").as_bytes())
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            while !another_descriptor_for(&manifest_lock) {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("real Rename must be blocked on the held manifest lock");
        if abort {
            let task = task.take().unwrap();
            task.abort();
            assert!(task.await.unwrap_err().is_cancelled());
        } else {
            shutdown.notify_one();
        }
        let mut reader = BufReader::new(client);
        let mut line = String::new();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), reader.read_line(&mut line))
                .await
                .expect("transport closes promptly while its effect drains")
                .unwrap(),
            0
        );
        if stopping == "timeout" {
            let result = tokio::time::timeout(Duration::from_secs(6), task.take().unwrap())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::TimedOut);
        }
        assert_eq!(
            work.drain_for(Duration::from_millis(10))
                .await
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::TimedOut
        );
        lifecycle.cleanup();
        assert!(
            socket.exists(),
            "cleanup must preserve the runtime artifacts of pending work"
        );
        assert!(crate::daemon_lifecycle::pid_path_for(&socket).exists());
        drop(lifecycle);
        assert_eq!(
            manifest::read_manifest(&root).unwrap().agents["ag-effect"].name,
            "before"
        );
        // EOF can precede the abortable dispatcher's guard drop. Observe the
        // ownership barrier across that asynchronous cancellation window.
        for _ in 0..20 {
            assert!(
                crate::daemon_lifecycle::try_acquire_daemon_lock(&socket)
                    .unwrap()
                    .is_none(),
                "successor must not acquire ownership while the old blocking effect is pending"
            );
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert_eq!(
            IpcServer::bind(&socket, Engine::new())
                .err()
                .unwrap()
                .kind(),
            std::io::ErrorKind::AddrInUse
        );

        drop(manifest_lock);
        if let Some(task) = task {
            task.await.unwrap().unwrap();
        }
        let successor = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                match IpcServer::bind(&socket, Engine::new()) {
                    Ok(server) => break server,
                    Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
                        tokio::time::sleep(Duration::from_millis(5)).await
                    }
                    Err(error) => panic!("unexpected successor bind error: {error}"),
                }
            }
        })
        .await
        .expect("ownership is released after the actual effect finishes");
        assert_eq!(
            manifest::read_manifest(&root).unwrap().agents["ag-effect"].name,
            "old-effect-completed"
        );
        assert_eq!(
            work.completed(),
            1,
            "cancelled waiters must leave a recorded completed operation"
        );
        drop(successor);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_completed_spawn_should_retain_ownership_for_its_later_blocked_exit_persistence() {
    use nix::fcntl::{Flock, FlockArg};
    use pu_core::{manifest, paths, types::AgentStatus};
    use std::time::Duration;
    let tmp = private_dir();
    let root = tmp.path().join("project");
    fs::create_dir(&root).unwrap();
    let engine = Engine::new();
    assert!(matches!(
        engine
            .handle_request(Request::Init {
                project_root: root.to_string_lossy().into_owned(),
            })
            .await,
        Response::InitResult { .. }
    ));
    let script = tmp.path().join("exit-on-release.sh");
    let release = tmp.path().join("release-child");
    // Natural exit only; the finite fallback also prevents a failed test from
    // stranding its scratch child or the runtime's PTY reader.
    fs::write(&script, "i=0\nwhile [ ! -f \"$1\" ] && [ \"$i\" -lt 2000 ]; do\n  sleep 0.01\n  i=$((i+1))\ndone\nexit 0\n").unwrap();
    let socket = tmp.path().join("daemon.sock");
    let server = IpcServer::bind(&socket, engine).unwrap();
    // Keep the old in-memory engine alive so its existing Drop cannot terminate
    // the scratch child. Its idle WorkOwner must not itself hold the flock.
    let engine = server.engine().clone();
    let work = server.work();
    let task = tokio::spawn(server.run());
    let mut client = tokio::net::UnixStream::connect(&socket).await.unwrap();
    let spawn = Request::Spawn {
        project_root: root.to_string_lossy().into_owned(),
        prompt: String::new(),
        agent: "terminal".into(),
        name: None,
        base: None,
        root: true,
        worktree: None,
        command: Some(format!(
            "/bin/sh {} {}",
            script.display(),
            release.display()
        )),
        no_auto: false,
        extra_args: vec![],
        plan_mode: false,
        no_trigger: true,
        trigger: None,
    };
    client
        .write_all((serde_json::to_string(&spawn).unwrap() + "\n").as_bytes())
        .await
        .unwrap();
    let mut client = BufReader::new(client);
    let mut line = String::new();
    client.read_line(&mut line).await.unwrap();
    let agent_id = match serde_json::from_str::<Response>(&line).unwrap() {
        Response::SpawnResult { agent_id, .. } => agent_id,
        other => panic!("unexpected spawn outcome: {other:?}"),
    };
    let file = fs::OpenOptions::new()
        .write(true)
        .open(paths::manifest_path(&root).with_extension("json.lock"))
        .unwrap();
    let manifest_lock = Flock::lock(file, FlockArg::LockExclusive).unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    line.clear();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(1), client.read_line(&mut line))
            .await
            .unwrap()
            .unwrap(),
        0
    );
    fs::write(&release, "exit naturally").unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while !another_descriptor_for(&manifest_lock) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("the detached natural-exit observer must reach its real manifest lock");
    assert!(
        crate::daemon_lifecycle::try_acquire_daemon_lock(&socket)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        work.drain_for(Duration::from_millis(20))
            .await
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::TimedOut
    );
    drop(manifest_lock);
    tokio::time::timeout(Duration::from_secs(3), work.wait_idle())
        .await
        .unwrap();
    assert_eq!(
        manifest::read_manifest(&root).unwrap().agents[&agent_id].status,
        AgentStatus::Broken
    );
    assert_eq!(
        work.completed(),
        2,
        "both the accepted spawn and its descendant reached outcomes"
    );
    let successor = IpcServer::bind(&socket, Engine::new()).unwrap();
    drop(successor);
    drop(engine);
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

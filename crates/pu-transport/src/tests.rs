use super::*;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

fn endpoint(dir: &tempfile::TempDir) -> Endpoint {
    Endpoint::for_socket(&dir.path().join("daemon.sock")).unwrap()
}

/// Serve one NDJSON echo connection per accept, reporting each peer.
fn spawn_echo(mut listener: Listener) -> tokio::sync::mpsc::UnboundedReceiver<PeerIdentity> {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(async move {
        loop {
            let Ok((stream, peer)) = listener.accept().await else {
                continue;
            };
            let _ = tx.send(peer);
            tokio::spawn(async move {
                let (r, mut w) = stream.into_split();
                let mut lines = BufReader::new(r).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let _ = w.write_all(format!("echo:{line}\n").as_bytes()).await;
                }
            });
        }
    });
    rx
}

#[tokio::test]
async fn given_listener_should_round_trip_a_line_and_report_same_user_peer() {
    let dir = tempfile::tempdir().unwrap();
    let ep = endpoint(&dir);
    let mut peers = spawn_echo(Listener::bind(&ep).unwrap());

    let stream = connect(&ep).await.unwrap();
    let (r, mut w) = stream.into_split();
    w.write_all(b"{\"type\":\"health\"}\n").await.unwrap();
    let mut line = String::new();
    BufReader::new(r).read_line(&mut line).await.unwrap();
    assert_eq!(line, "echo:{\"type\":\"health\"}\n");

    let peer = peers.recv().await.unwrap();
    assert!(peer.is_same_user());
}

#[tokio::test]
async fn given_many_sequential_clients_should_serve_each() {
    let dir = tempfile::tempdir().unwrap();
    let ep = endpoint(&dir);
    let _peers = spawn_echo(Listener::bind(&ep).unwrap());
    for i in 0..20 {
        let (r, mut w) = connect(&ep).await.unwrap().into_split();
        w.write_all(format!("{i}\n").as_bytes()).await.unwrap();
        let mut line = String::new();
        BufReader::new(r).read_line(&mut line).await.unwrap();
        assert_eq!(line, format!("echo:{i}\n"));
    }
}

#[tokio::test]
async fn given_no_daemon_should_fail_to_connect() {
    let dir = tempfile::tempdir().unwrap();
    assert!(connect(&endpoint(&dir)).await.is_err());
}

#[cfg(windows)]
#[tokio::test]
async fn given_missing_home_connect_should_not_create_it_but_bind_should() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("absent").join(".pu");
    let ep = Endpoint::for_socket(&home.join("daemon.sock")).unwrap();
    assert!(connect(&ep).await.is_err());
    assert!(!dir.path().join("absent").exists(), "client created a dir");
    let _peers = spawn_echo(Listener::bind(&ep).unwrap());
    assert!(home.is_dir());
    // The name computed before the directory existed is the bound one.
    let again = Endpoint::for_socket(&home.join("daemon.sock")).unwrap();
    assert_eq!(again, ep);
    connect(&again).await.unwrap();
}

#[tokio::test]
async fn given_peer_hangs_up_should_report_closed_without_consuming() {
    let dir = tempfile::tempdir().unwrap();
    let ep = endpoint(&dir);
    let mut listener = Listener::bind(&ep).unwrap();
    let client = tokio::spawn({
        let ep = ep.clone();
        async move { connect(&ep).await.unwrap() }
    });
    let (server, _) = listener.accept().await.unwrap();
    let client = client.await.unwrap();
    let (r, _w) = server.into_split();
    assert!(!r.peer_sent_or_closed(), "idle connection reported closed");
    drop(client);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while !r.peer_sent_or_closed() {
        assert!(std::time::Instant::now() < deadline, "hang-up never seen");
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
}

#[tokio::test]
async fn given_peer_sends_bytes_should_report_them_and_still_deliver_them() {
    let dir = tempfile::tempdir().unwrap();
    let ep = endpoint(&dir);
    let mut listener = Listener::bind(&ep).unwrap();
    let client = tokio::spawn({
        let ep = ep.clone();
        async move { connect(&ep).await.unwrap() }
    });
    let (server, _) = listener.accept().await.unwrap();
    let mut client = client.await.unwrap();
    client.write_all(b"abc").await.unwrap();
    let (mut r, _w) = server.into_split();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while !r.peer_sent_or_closed() {
        assert!(std::time::Instant::now() < deadline, "bytes never seen");
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let mut buf = [0u8; 3];
    r.read_exact(&mut buf).await.unwrap();
    assert_eq!(&buf, b"abc");
}

#[cfg(windows)]
#[tokio::test]
async fn given_pipe_name_already_owned_should_refuse_to_bind() {
    let dir = tempfile::tempdir().unwrap();
    let ep = endpoint(&dir);
    let _first = Listener::bind(&ep).unwrap();
    let err = Listener::bind(&ep).err().expect("second bind must fail");
    assert_eq!(err.kind(), std::io::ErrorKind::AddrInUse);
}

#[cfg(windows)]
#[tokio::test]
async fn given_pid_file_naming_another_live_process_should_refuse_the_server() {
    // A same-user impostor passes the SID check; the daemon pid file must
    // catch it. Here our own listener plays the impostor and the pid file
    // names a different live process (a child we spawn).
    let dir = tempfile::tempdir().unwrap();
    let ep = endpoint(&dir);
    let _peers = spawn_echo(Listener::bind(&ep).unwrap());
    let mut child = std::process::Command::new("cmd")
        .args(["/c", "ping", "-n", "5", "127.0.0.1"])
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    std::fs::write(ep.pid_path(), child.id().to_string()).unwrap();
    let err = connect(&ep).await.err().expect("impostor must be refused");
    assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
    let _ = child.kill();
    let _ = child.wait();
    // A stale pid file (process gone) does not block the real daemon.
    connect(&ep).await.unwrap();
}

#[cfg(windows)]
#[tokio::test]
async fn given_pid_file_naming_this_process_should_connect() {
    let dir = tempfile::tempdir().unwrap();
    let ep = endpoint(&dir);
    let _peers = spawn_echo(Listener::bind(&ep).unwrap());
    std::fs::write(ep.pid_path(), std::process::id().to_string()).unwrap();
    connect(&ep).await.unwrap();
}

#[test]
fn given_peer_uid_should_allow_only_the_daemon_user() {
    assert!(same_user(Some(501), 501));
    assert!(!same_user(Some(502), 501));
    assert!(!same_user(Some(0), 501), "root is not the daemon user");
    assert!(!same_user(None, 501), "unknown peer");
}

#[cfg(windows)]
#[test]
fn given_server_token_trust_should_refuse_lower_integrity_and_sandboxed_servers() {
    use win_sys::{TokenTrust, untrusted_server_reason};
    const LOW: u32 = 0x1000;
    const MEDIUM: u32 = 0x2000;
    const HIGH: u32 = 0x3000;
    let plain = |integrity| TokenTrust {
        integrity,
        app_container: false,
        restricted: false,
    };
    assert_eq!(untrusted_server_reason(plain(MEDIUM), MEDIUM), None);
    assert_eq!(untrusted_server_reason(plain(HIGH), MEDIUM), None);
    assert!(untrusted_server_reason(plain(LOW), MEDIUM).is_some());
    // An elevated client (admin terminal) trusts the user's normal daemon...
    assert_eq!(untrusted_server_reason(plain(MEDIUM), HIGH), None);
    // ...but never one below Medium.
    assert!(untrusted_server_reason(plain(LOW), HIGH).is_some());
    let sandboxed = TokenTrust {
        app_container: true,
        ..plain(HIGH)
    };
    assert!(untrusted_server_reason(sandboxed, MEDIUM).is_some());
    let restricted = TokenTrust {
        restricted: true,
        ..plain(HIGH)
    };
    assert!(untrusted_server_reason(restricted, MEDIUM).is_some());
}

#[cfg(windows)]
#[test]
fn given_own_process_should_pass_server_trust_check() {
    let me = win_sys::current_token_trust().unwrap();
    let same = win_sys::process_trust(std::process::id()).unwrap();
    assert_eq!(me, same);
    assert!(!me.app_container && !me.restricted, "{me:?}");
    assert!(me.integrity >= 0x2000, "{me:?}");
    assert_eq!(win_sys::untrusted_server_reason(same, me.integrity), None);
}

#[tokio::test]
async fn given_try_write_should_send_without_waiting() {
    let dir = tempfile::tempdir().unwrap();
    let ep = endpoint(&dir);
    let mut listener = Listener::bind(&ep).unwrap();
    let client = tokio::spawn({
        let ep = ep.clone();
        async move { connect(&ep).await.unwrap() }
    });
    let (server, _) = listener.accept().await.unwrap();
    let client = client.await.unwrap();
    // A freshly accepted pipe may not be writable yet (WIN-1.2): retry briefly.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        match server.try_write(b"BUSY\n") {
            Ok(_) => break,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(std::time::Instant::now() < deadline);
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
            Err(e) => panic!("{e}"),
        }
    }
    let mut line = String::new();
    BufReader::new(client).read_line(&mut line).await.unwrap();
    assert_eq!(line, "BUSY\n");
}

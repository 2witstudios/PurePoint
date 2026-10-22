use std::io::Write;
use std::os::unix::net::UnixDatagram;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[test]
fn given_isolated_observer_should_relay_without_daemon_or_stdout() {
    let home = tempfile::tempdir().unwrap();
    let path = home.path().join("s");
    let socket = UnixDatagram::bind(&path).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_pu"))
        .arg("__agent-signal")
        .env_clear()
        .env("HOME", home.path())
        .env("PU_SIGNAL_SOCKET", &path)
        .env("PU_SIGNAL_TOKEN", "private")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(br#"{"session_id":"exact","hook_event_name":"UserPromptSubmit"}"#)
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    assert!(out.stdout.is_empty());
    assert!(out.stderr.is_empty());
    let mut data = [0; 1024];
    let n = socket.recv(&mut data).unwrap();
    let frame: serde_json::Value = serde_json::from_slice(&data[..n]).unwrap();
    assert_eq!(frame["payload"]["session_id"], "exact");
    assert_eq!(frame["token"], "private");
    assert!(!home.path().join(".pu").exists());
}

#[test]
fn given_dead_listener_or_open_stdin_should_exit_success_within_budget() {
    let home = tempfile::tempdir().unwrap();
    // Match production readiness preflight; cold OS validation is recorded as
    // an unproved invocation budget, not hidden inside a warm relay assertion.
    assert!(
        Command::new(env!("CARGO_BIN_EXE_pu"))
            .arg("__agent-signal")
            .env_clear()
            .env("HOME", home.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success()
    );
    for open in [false, true] {
        let start = Instant::now();
        let mut child = Command::new(env!("CARGO_BIN_EXE_pu"))
            .arg("__agent-signal")
            .env_clear()
            .env("HOME", home.path())
            .env("PU_SIGNAL_SOCKET", home.path().join("absent"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        if !open {
            drop(input);
        }
        let out = child.wait_with_output().unwrap();
        assert!(start.elapsed() < Duration::from_millis(500));
        assert!(out.status.success());
        assert!(out.stdout.is_empty());
        assert!(out.stderr.is_empty());
    }
}

#[test]
fn given_full_listener_should_drop_without_waiting_or_printing_decisions() {
    let home = tempfile::tempdir().unwrap();
    let path = home.path().join("s");
    let _listener = UnixDatagram::bind(&path).unwrap();
    let fill = UnixDatagram::unbound().unwrap();
    fill.set_nonblocking(true).unwrap();
    for _ in 0..10000 {
        if fill.send_to(&[0; 1024], &path).is_err() {
            break;
        }
    }
    // Production launch preparation preflights this helper too. On macOS the
    // first execution after relinking includes OS code-signature verification.
    assert!(
        Command::new(env!("CARGO_BIN_EXE_pu"))
            .arg("__agent-signal")
            .env_clear()
            .env("HOME", home.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success()
    );
    let start = Instant::now();
    let mut child = Command::new(env!("CARGO_BIN_EXE_pu"))
        .arg("__agent-signal")
        .env_clear()
        .env("HOME", home.path())
        .env("PU_SIGNAL_SOCKET", path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"{}").unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    assert!(out.stdout.is_empty());
    assert!(out.stderr.is_empty());
    assert!(start.elapsed() < Duration::from_millis(250));
}

#[test]
fn given_oversized_or_malformed_payload_should_fail_open_without_relay() {
    let home = tempfile::tempdir().unwrap();
    let path = home.path().join("s");
    let listener = UnixDatagram::bind(&path).unwrap();
    listener.set_nonblocking(true).unwrap();
    for payload in [vec![b'x'; 256 * 1024 + 1], b"not json".to_vec()] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_pu"))
            .arg("__agent-signal")
            .env_clear()
            .env("HOME", home.path())
            .env("PU_SIGNAL_SOCKET", &path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let _ = child.stdin.take().unwrap().write_all(&payload);
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success());
        assert!(out.stdout.is_empty());
        assert!(out.stderr.is_empty());
        assert!(listener.recv(&mut [0; 1024]).is_err());
    }
}

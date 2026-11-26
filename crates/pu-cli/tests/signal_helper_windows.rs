// Relays over the per-launch signal named pipe (WIN-3.9).
#![cfg(windows)]

use std::io::Write;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use pu_core::signals::{ObservationState, SignalActivity};
use pu_engine::signals::testing::HookIntake;
use pu_transport::signal::{MAX_FRAME, SignalListener};

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap()
}

fn pipe_name(tag: &str) -> String {
    let home = tempfile::tempdir().unwrap();
    let nonce = format!("{tag}-{}-{}", std::process::id(), pu_core::id::session_id());
    pu_transport::signal::pipe_name(&home.path().join("daemon.sock"), &nonce).unwrap()
}

/// Run the helper the way a hook does: payload on stdin, signal env only.
fn helper(socket: Option<&str>, token: &str, stdin: Option<&[u8]>) -> Output {
    let home = tempfile::tempdir().unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_pu"));
    command
        .arg("__agent-signal")
        .env_clear()
        .env("SystemRoot", std::env::var_os("SystemRoot").unwrap())
        .env("HOME", home.path())
        .env("PU_SIGNAL_TOKEN", token)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(socket) = socket {
        command.env("PU_SIGNAL_SOCKET", socket);
    }
    let mut child = command.spawn().unwrap();
    let input = child.stdin.take().unwrap();
    let _keep_open;
    if let Some(bytes) = stdin {
        let mut input = input;
        let _ = input.write_all(bytes);
    } else {
        // An open, silent stdin: the helper must still finish on time.
        _keep_open = input;
    }
    let out = child.wait_with_output().unwrap();
    assert!(!home.path().join(".pu").exists());
    out
}

fn assert_silent_success(out: &Output) {
    assert!(out.status.success(), "{:?}", out.status);
    assert!(out.stdout.is_empty());
    assert!(out.stderr.is_empty());
}

#[test]
fn given_isolated_observer_should_relay_one_frame_over_the_pipe() {
    let rt = runtime();
    let name = pipe_name("relay");
    let listener = rt.block_on(async { SignalListener::bind(&name) }).unwrap();
    // Warm the binary (first-run image validation is outside the budget).
    assert_silent_success(&helper(None, "", Some(b"")));
    let out = helper(
        Some(&name),
        "private",
        Some(br#"{"session_id":"exact","hook_event_name":"UserPromptSubmit"}"#),
    );
    assert_silent_success(&out);
    let mut data = vec![0; MAX_FRAME];
    let n = rt
        .block_on(async {
            tokio::time::timeout(Duration::from_secs(2), listener.recv(&mut data)).await
        })
        .unwrap()
        .unwrap();
    let frame: serde_json::Value = serde_json::from_slice(&data[..n]).unwrap();
    assert_eq!(frame["payload"]["session_id"], "exact");
    assert!(frame["payload"]["occurred_at_ms"].is_i64());
    assert_eq!(frame["token"], "private");
}

#[test]
fn given_dead_listener_or_open_stdin_should_exit_success_within_budget() {
    assert_silent_success(&helper(None, "", Some(b"")));
    let absent = pipe_name("absent");
    for stdin in [Some(&b"{}"[..]), None] {
        let start = Instant::now();
        let out = helper(Some(&absent), "", stdin);
        assert!(
            start.elapsed() < Duration::from_millis(500),
            "{:?}",
            start.elapsed()
        );
        assert_silent_success(&out);
    }
}

#[test]
fn given_oversized_or_malformed_payload_should_fail_open_without_relay() {
    let rt = runtime();
    let name = pipe_name("malformed");
    let listener = rt.block_on(async { SignalListener::bind(&name) }).unwrap();
    for payload in [vec![b'x'; 256 * 1024 + 1], b"not json".to_vec()] {
        assert_silent_success(&helper(Some(&name), "", Some(&payload)));
    }
    let mut data = vec![0; MAX_FRAME];
    let got = rt.block_on(async {
        tokio::time::timeout(Duration::from_millis(300), listener.recv(&mut data)).await
    });
    assert!(got.is_err(), "nothing may be relayed");
}

/// A scripted hook sequence through the real helper reaches the engine's
/// launch intake and moves the agent's observed state as on Unix: the pi
/// registration binds the session, then `agent_start` is Working.
#[test]
fn given_scripted_hooks_through_helper_should_move_engine_state() {
    let rt = runtime();
    let intake = rt
        .block_on(async { HookIntake::start("pi", None, 4242) })
        .unwrap();
    assert!(intake.snapshot().is_none());
    assert_silent_success(&helper(None, "", Some(b"")));
    let now = chrono::Utc::now().timestamp_millis();
    for (sequence, kind) in [(1, "observer_bound"), (2, "agent_start")] {
        let payload = serde_json::json!({
            "session_id": "root", "native_pid": 4242, "sequence": sequence,
            "type": kind, "occurred_at_ms": now,
        });
        let out = helper(
            Some(&intake.address),
            &intake.token,
            Some(&serde_json::to_vec(&payload).unwrap()),
        );
        assert_silent_success(&out);
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    let snapshot = loop {
        if let Some(s) = intake.snapshot()
            && s.activity == SignalActivity::Working
        {
            break s;
        }
        assert!(Instant::now() < deadline, "{:?}", intake.snapshot());
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(snapshot.observation.state, ObservationState::Current);

    // A wrong token through the same helper changes nothing.
    let payload = serde_json::json!({
        "session_id": "root", "native_pid": 4242, "sequence": 3,
        "type": "agent_settled", "outcome": "completed", "aborted": false,
        "occurred_at_ms": chrono::Utc::now().timestamp_millis(),
    });
    let out = helper(
        Some(&intake.address),
        "wrong-token",
        Some(&serde_json::to_vec(&payload).unwrap()),
    );
    assert_silent_success(&out);
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(intake.snapshot().unwrap().activity, SignalActivity::Working);
}

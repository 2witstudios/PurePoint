use std::path::{Path, PathBuf};

use pu_core::protocol::{Replay, Request, Response};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;
use tokio::time::timeout;

use super::*;
use crate::daemon::client::parse;

const WAIT: Duration = Duration::from_secs(10);

/// What a test sink saw, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Seen {
    Event(AttachEvent),
    Output(Vec<u8>),
}

struct TestSink(mpsc::UnboundedSender<Seen>);

impl AttachSink for TestSink {
    fn event(&self, event: AttachEvent) {
        let _ = self.0.send(Seen::Event(event));
    }
    fn output(&self, bytes: Vec<u8>) {
        let _ = self.0.send(Seen::Output(bytes));
    }
}

fn sink() -> (Arc<dyn AttachSink>, mpsc::UnboundedReceiver<Seen>) {
    let (tx, rx) = mpsc::unbounded_channel();
    (Arc::new(TestSink(tx)), rx)
}

async fn next(seen: &mut mpsc::UnboundedReceiver<Seen>) -> Seen {
    timeout(WAIT, seen.recv())
        .await
        .expect("timed out waiting for the session")
        .expect("session sink closed")
}

async fn next_event(seen: &mut mpsc::UnboundedReceiver<Seen>) -> AttachEvent {
    match next(seen).await {
        Seen::Event(event) => event,
        other => panic!("expected an event, got {other:?}"),
    }
}

/// One accepted connection on the fake daemon: the requests it sent and its write half.
/// Dropping it closes the pipe: the read half lives in `_reader`, which is aborted then.
struct ServerConn {
    requests: mpsc::UnboundedReceiver<Request>,
    writer: pu_transport::WriteHalf,
    _reader: TaskGuard,
}

impl ServerConn {
    async fn request(&mut self) -> Request {
        timeout(WAIT, self.requests.recv())
            .await
            .expect("timed out waiting for a request")
            .expect("client closed the connection")
    }

    async fn send(&mut self, response: &Response) {
        let mut line = serde_json::to_vec(response).unwrap();
        line.push(b'\n');
        self.writer.write_all(&line).await.unwrap();
        self.writer.flush().await.unwrap();
    }

    async fn send_raw(&mut self, bytes: &[u8]) {
        self.writer.write_all(bytes).await.unwrap();
    }
}

/// A fake daemon on a `pu_transport::Listener` handing each connection to the test.
struct FakeDaemon {
    connections: mpsc::UnboundedReceiver<ServerConn>,
    task: tokio::task::JoinHandle<()>,
    _home: tempfile::TempDir,
    socket: PathBuf,
}

impl FakeDaemon {
    fn start() -> Self {
        let home = tempfile::TempDir::new().unwrap();
        let socket = home.path().join("d.sock");
        let endpoint = pu_transport::Endpoint::for_socket(&socket).unwrap();
        let mut listener = pu_transport::Listener::bind(&endpoint).unwrap();
        let (tx, connections) = mpsc::unbounded_channel();
        let task = tokio::spawn(async move {
            loop {
                let Ok((stream, _peer)) = listener.accept().await else {
                    continue;
                };
                let (read, writer) = stream.into_split();
                let (requests_tx, requests) = mpsc::unbounded_channel();
                let reader = tokio::spawn(async move {
                    let mut lines = BufReader::new(read).lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        if requests_tx
                            .send(serde_json::from_str(&line).unwrap())
                            .is_err()
                        {
                            break;
                        }
                    }
                });
                if tx
                    .send(ServerConn {
                        requests,
                        writer,
                        _reader: TaskGuard(reader),
                    })
                    .is_err()
                {
                    return;
                }
            }
        });
        Self {
            connections,
            task,
            _home: home,
            socket,
        }
    }

    fn client(&self) -> DaemonClient {
        DaemonClient::local(&self.socket).unwrap()
    }

    async fn accept(&mut self) -> ServerConn {
        timeout(WAIT, self.connections.recv())
            .await
            .expect("timed out waiting for a connection")
            .unwrap()
    }

    /// Accept a connection and check its attach request; returns the client id.
    async fn accept_attach(&mut self, agent: &str) -> (ServerConn, String) {
        let mut conn = self.accept().await;
        match conn.request().await {
            Request::Attach {
                agent_id,
                replay,
                client_id: Some(client_id),
            } => {
                assert_eq!(agent_id, agent);
                assert_eq!(replay, Some(Replay::Snapshot));
                (conn, client_id)
            }
            other => panic!("expected attach, got {other:?}"),
        }
    }

    async fn assert_no_connection(&mut self, within: Duration) {
        assert!(
            timeout(within, self.connections.recv()).await.is_err(),
            "the session reconnected"
        );
    }
}

impl Drop for FakeDaemon {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// `Request` has no `PartialEq`; compare the wire form.
fn assert_request(got: Request, want: Request) {
    assert_eq!(
        serde_json::to_value(got).unwrap(),
        serde_json::to_value(want).unwrap()
    );
}

fn output(data: &[u8]) -> Response {
    Response::Output {
        agent_id: "ag-1".into(),
        data: data.to_vec(),
    }
}

fn fast_backoff(max_retries: u32) -> Backoff {
    Backoff {
        fast_retries: 2,
        fast_delay: Duration::from_millis(10),
        slow_delay: Duration::from_millis(40),
        max_retries,
    }
}

#[test]
fn given_swift_backoff_should_retry_five_times_fast_then_slow_up_to_twenty() {
    let delays: Vec<_> = (1..=21).map(|n| Backoff::SWIFT.delay(n)).collect();
    assert!(
        delays[..5]
            .iter()
            .all(|d| *d == Some(Duration::from_millis(100)))
    );
    assert!(
        delays[5..20]
            .iter()
            .all(|d| *d == Some(Duration::from_secs(2)))
    );
    assert_eq!(delays[20], None);
}

#[tokio::test]
async fn given_attach_should_forward_ready_replay_live_resync_and_sizes_in_order() {
    let mut daemon = FakeDaemon::start();
    let (sink, mut seen) = sink();
    let session = AttachSession::start(daemon.client(), AttachOptions::new("ag-1", 100, 30), sink);
    let (mut conn, client_id) = daemon.accept_attach("ag-1").await;
    assert!(client_id.starts_with("win:"), "{client_id}");
    assert_eq!(client_id, session.client_id());
    assert_eq!(client_id.len(), "win:".len() + 36);

    conn.send(&Response::AttachReady { buffered_bytes: 5 })
        .await;
    conn.send(&Response::PtySize {
        cols: 80,
        rows: 24,
        owner_client_id: Some(client_id.clone()),
    })
    .await;
    conn.send(&output(b"hel")).await;
    conn.send(&output(b"lo")).await;
    conn.send(&output(b"\x1b[31mlive")).await;
    conn.send(&Response::Resync).await;
    conn.send(&Response::PtySize {
        cols: 90,
        rows: 20,
        owner_client_id: Some("mac:other".into()),
    })
    .await;

    let expected = [
        Seen::Event(AttachEvent::AttachReady { buffered_bytes: 5 }),
        Seen::Event(AttachEvent::PtySize {
            cols: 80,
            rows: 24,
            owner_client_id: Some(client_id.clone()),
            owned: true,
        }),
        Seen::Output(b"hel".to_vec()),
        Seen::Output(b"lo".to_vec()),
        Seen::Event(AttachEvent::Live),
        Seen::Output(b"\x1b[31mlive".to_vec()),
        Seen::Event(AttachEvent::Resync),
        Seen::Event(AttachEvent::PtySize {
            cols: 90,
            rows: 20,
            owner_client_id: Some("mac:other".into()),
            owned: false,
        }),
    ];
    for want in expected {
        assert_eq!(next(&mut seen).await, want);
    }

    // Input, resize and claim_size go out on the attach connection itself.
    session.input(b"ls\r".to_vec());
    session.resize(120, 40);
    session.resize(0, 40);
    session.claim_size();
    assert!(session.deliver(b"/tmp/a.png".to_vec(), WAIT).await);
    let input = |data: &[u8]| Request::Input {
        agent_id: "ag-1".into(),
        data: data.to_vec(),
        submit: false,
        cancel_on_disconnect: false,
    };
    assert_request(conn.request().await, input(b"ls\r"));
    assert_request(
        conn.request().await,
        Request::Resize {
            agent_id: "ag-1".into(),
            cols: 120,
            rows: 40,
        },
    );
    assert_request(
        conn.request().await,
        Request::ClaimSize {
            cols: 120,
            rows: 40,
        },
    );
    assert_request(conn.request().await, input(b"/tmp/a.png"));

    // End of stream after attach_ready: the agent exited, so no reattach.
    drop(conn);
    assert_eq!(next_event(&mut seen).await, AttachEvent::Exited);
    daemon
        .assert_no_connection(Duration::from_millis(300))
        .await;
    assert!(!session.deliver(b"late".to_vec(), WAIT).await);
}

#[tokio::test]
async fn given_focused_pane_should_claim_its_size_after_attach_ready() {
    let mut daemon = FakeDaemon::start();
    let (sink, mut seen) = sink();
    let mut options = AttachOptions::new("ag-1", 132, 43);
    options.focused = true;
    let _session = AttachSession::start(daemon.client(), options, sink);
    let (mut conn, _) = daemon.accept_attach("ag-1").await;
    conn.send(&Response::AttachReady { buffered_bytes: 0 })
        .await;
    assert_eq!(
        next_event(&mut seen).await,
        AttachEvent::AttachReady { buffered_bytes: 0 }
    );
    assert_eq!(next_event(&mut seen).await, AttachEvent::Live);
    assert_request(
        conn.request().await,
        Request::ClaimSize {
            cols: 132,
            rows: 43,
        },
    );
}

#[tokio::test]
async fn given_agent_not_found_should_report_gone_without_retrying() {
    let mut daemon = FakeDaemon::start();
    let (sink, mut seen) = sink();
    let _session = AttachSession::start(daemon.client(), AttachOptions::new("ag-1", 80, 24), sink);
    let (mut conn, _) = daemon.accept_attach("ag-1").await;
    conn.send(&Response::Error {
        code: "AGENT_NOT_FOUND".into(),
        message: "agent ag-1 not found".into(),
    })
    .await;
    assert_eq!(next_event(&mut seen).await, AttachEvent::Gone);
    daemon
        .assert_no_connection(Duration::from_millis(300))
        .await;
}

#[tokio::test]
async fn given_failures_should_back_off_then_give_up() {
    let mut daemon = FakeDaemon::start();
    let (sink, mut seen) = sink();
    let mut options = AttachOptions::new("ag-1", 80, 24);
    options.backoff = fast_backoff(4);
    let _session = AttachSession::start(daemon.client(), options, sink);

    let started = tokio::time::Instant::now();
    let mut gaps = Vec::new();
    // Closed before attach_ready, BUSY, another error, garbage, then closed again: all retried.
    for attempt in 1..=5u32 {
        let (mut conn, _) = daemon.accept_attach("ag-1").await;
        gaps.push(started.elapsed());
        match attempt {
            2 => {
                conn.send(&Response::Error {
                    code: "BUSY".into(),
                    message: "too many connections".into(),
                })
                .await
            }
            3 => {
                conn.send(&Response::Error {
                    code: "INTERNAL".into(),
                    message: "boom".into(),
                })
                .await
            }
            4 => conn.send_raw(b"{not json\n").await,
            _ => {}
        }
        drop(conn);
        let event = next_event(&mut seen).await;
        if attempt == 5 {
            assert!(
                matches!(&event, AttachEvent::Failed { .. }),
                "after the last retry: {event:?}"
            );
            break;
        }
        let AttachEvent::Retrying {
            attempt: n,
            delay_ms,
            message,
        } = event
        else {
            panic!("expected a retry, got {event:?}");
        };
        assert_eq!(n, attempt);
        assert_eq!(delay_ms, if attempt <= 2 { 10 } else { 40 });
        if attempt == 3 {
            assert_eq!(message, "Attach failed: boom");
        }
    }
    daemon
        .assert_no_connection(Duration::from_millis(200))
        .await;
    // The slow retries waited their 40 ms.
    assert!(gaps[4] - gaps[3] >= Duration::from_millis(35), "{gaps:?}");
}

#[tokio::test]
async fn given_reattach_should_reset_before_the_replay() {
    let mut daemon = FakeDaemon::start();
    let (sink, mut seen) = sink();
    let mut options = AttachOptions::new("ag-1", 80, 24);
    options.backoff = fast_backoff(3);
    let _session = AttachSession::start(daemon.client(), options, sink);

    let (mut conn, _) = daemon.accept_attach("ag-1").await;
    conn.send(&Response::AttachReady { buffered_bytes: 3 })
        .await;
    conn.send(&output(b"abc")).await;
    // A mid-stream error is a failure, not an exit.
    conn.send(&Response::Error {
        code: "FRAME_TOO_LARGE".into(),
        message: "too large".into(),
    })
    .await;
    assert_eq!(
        next_event(&mut seen).await,
        AttachEvent::AttachReady { buffered_bytes: 3 }
    );
    assert_eq!(next(&mut seen).await, Seen::Output(b"abc".to_vec()));
    assert_eq!(next_event(&mut seen).await, AttachEvent::Live);
    assert!(matches!(
        next_event(&mut seen).await,
        AttachEvent::Retrying { attempt: 1, .. }
    ));

    let (mut conn, _) = daemon.accept_attach("ag-1").await;
    conn.send(&Response::AttachReady { buffered_bytes: 3 })
        .await;
    conn.send(&output(b"abc")).await;
    assert_eq!(
        next_event(&mut seen).await,
        AttachEvent::AttachReady { buffered_bytes: 3 }
    );
    assert_eq!(next_event(&mut seen).await, AttachEvent::Reset);
    assert_eq!(next(&mut seen).await, Seen::Output(b"abc".to_vec()));
}

#[tokio::test]
async fn given_pane_with_content_should_reset_before_the_first_replay() {
    let mut daemon = FakeDaemon::start();
    let (sink, mut seen) = sink();
    let mut options = AttachOptions::new("ag-1", 80, 24);
    options.reset_before_replay = true;
    let _session = AttachSession::start(daemon.client(), options, sink);
    let (mut conn, _) = daemon.accept_attach("ag-1").await;
    conn.send(&Response::AttachReady { buffered_bytes: 1 })
        .await;
    conn.send(&output(b"x")).await;
    next_event(&mut seen).await;
    assert_eq!(next_event(&mut seen).await, AttachEvent::Reset);
    assert_eq!(next(&mut seen).await, Seen::Output(b"x".to_vec()));
}

#[tokio::test]
async fn given_dropped_session_should_close_its_connection() {
    let mut daemon = FakeDaemon::start();
    let (sink, _seen) = sink();
    let session = AttachSession::start(daemon.client(), AttachOptions::new("ag-1", 80, 24), sink);
    let (mut conn, _) = daemon.accept_attach("ag-1").await;
    conn.send(&Response::AttachReady { buffered_bytes: 0 })
        .await;
    drop(session);
    assert!(
        timeout(WAIT, conn.requests.recv()).await.unwrap().is_none(),
        "the connection stayed open"
    );
}

#[test]
fn given_output_lines_should_decode_fast_or_fall_back() {
    let line = serde_json::to_vec(&output(b"\x00\x1b[1m\xff")).unwrap();
    let Some(Response::Output { agent_id, data }) = fast_output(&line) else {
        panic!("not decoded");
    };
    assert_eq!(
        (agent_id.as_str(), data.as_slice()),
        ("ag-1", &b"\x00\x1b[1m\xff"[..])
    );
    // Anything else goes to serde: other field orders, escapes, odd or bad hex, other types.
    for other in [
        &br#"{"type":"output","data":"41","agent_id":"ag-1"}"#[..],
        br#"{"type":"output","agent_id":"a\"b","data":"41"}"#,
        br#"{"type":"output","agent_id":"ag-1","data":"414"}"#,
        br#"{"type":"output","agent_id":"ag-1","data":"zz"}"#,
        br#"{"type":"resync"}"#,
    ] {
        assert!(
            fast_output(other).is_none(),
            "{}",
            String::from_utf8_lossy(other)
        );
    }
    assert!(matches!(
        parse(br#"{"type":"output","data":"41","agent_id":"ag-1"}"#),
        Ok(Response::Output { data, .. }) if data == b"A"
    ));
}

/// FNV-1a, enough to tell a reordered or corrupted stream from the original.
fn fnv1a(hash: u64, bytes: &[u8]) -> u64 {
    bytes.iter().fold(hash, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3)
    })
}
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// An `output` line for `data`, hex-encoded with a table (the test's cost, not the session's).
fn output_line(data: &[u8]) -> Vec<u8> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut line = br#"{"type":"output","agent_id":"ag-1","data":""#.to_vec();
    line.reserve(data.len() * 2 + 3);
    for b in data {
        line.push(HEX[usize::from(b >> 4)]);
        line.push(HEX[usize::from(b & 0xf)]);
    }
    line.extend_from_slice(b"\"}\n");
    line
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn given_50mb_stream_should_deliver_every_byte_in_order() {
    const TOTAL: usize = 50 * 1024 * 1024;
    const CHUNK: usize = 64 * 1024;

    // Eight distinct chunks of every byte value, so order and content both matter.
    let mut state: u32 = 0x2545_f491;
    let chunks: Vec<Vec<u8>> = (0..8)
        .map(|_| {
            (0..CHUNK)
                .map(|_| {
                    state ^= state << 13;
                    state ^= state >> 17;
                    state ^= state << 5;
                    state as u8
                })
                .collect()
        })
        .collect();
    let lines: Vec<Vec<u8>> = chunks.iter().map(|c| output_line(c)).collect();
    let count = TOTAL / CHUNK;
    let expected_hash = (0..count).fold(FNV_OFFSET, |h, i| fnv1a(h, &chunks[i % 8]));

    let mut daemon = FakeDaemon::start();
    let (sink, mut seen) = sink();
    let _session = AttachSession::start(daemon.client(), AttachOptions::new("ag-1", 80, 24), sink);
    let (mut conn, _) = daemon.accept_attach("ag-1").await;
    let started = std::time::Instant::now();
    let writer = tokio::spawn(async move {
        conn.send(&Response::AttachReady { buffered_bytes: 0 })
            .await;
        for i in 0..count {
            conn.send_raw(&lines[i % 8]).await;
        }
        conn.writer.flush().await.unwrap();
        conn
    });

    let (mut received, mut hash) = (0usize, FNV_OFFSET);
    while received < TOTAL {
        match timeout(Duration::from_secs(120), seen.recv())
            .await
            .unwrap()
        {
            Some(Seen::Output(bytes)) => {
                received += bytes.len();
                hash = fnv1a(hash, &bytes);
            }
            Some(Seen::Event(AttachEvent::AttachReady { .. } | AttachEvent::Live)) => {}
            other => panic!("unexpected {other:?}"),
        }
    }
    let elapsed = started.elapsed();
    let _conn = writer.await.unwrap();
    assert_eq!(received, TOTAL);
    assert_eq!(hash, expected_hash);
    eprintln!(
        "attach throughput: {} MiB in {:.2?} ({:.1} MiB/s, fnv1a {hash:016x})",
        TOTAL >> 20,
        elapsed,
        (TOTAL >> 20) as f64 / elapsed.as_secs_f64()
    );
}

/// The workspace-built binary `name` (`cargo build -p pu-engine -p pu-cli` at the root).
fn built(name: &str) -> Option<PathBuf> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../target/debug")
        .join(name);
    path.is_file()
        .then(|| pu_core::canon::canonicalize(&path).unwrap_or(path))
}

#[cfg(windows)]
#[tokio::test(flavor = "multi_thread")]
async fn given_real_daemon_should_stream_a_shell_and_apply_resizes() {
    use crate::daemon::lifecycle::{BinaryLocator, DaemonLifecycle};
    use std::process::Command;

    let (Some(engine), Some(pu)) = (built("pu-engine.exe"), built("pu.exe")) else {
        eprintln!("skipped: run `cargo build -p pu-engine -p pu-cli` at the repo root first");
        return;
    };
    let home = tempfile::TempDir::new().unwrap();
    let home_path = pu_core::canon::canonicalize(home.path()).unwrap();
    let socket = home_path.join(".pu").join("daemon.sock");
    let env = [("HOME", &home_path), ("USERPROFILE", &home_path)];
    let lifecycle = DaemonLifecycle::new(
        &socket,
        BinaryLocator {
            dirs: vec![engine.parent().unwrap().to_path_buf()],
            home: None,
            explicit: None,
        },
    )
    .unwrap()
    .with_spawn_env(env.iter().map(|(k, v)| ((*k).into(), v.into())).collect());
    lifecycle.ensure_daemon().await.unwrap_or_else(|e| {
        let log = std::fs::read_to_string(home_path.join(".pu").join("daemon.log"));
        panic!("{e}; daemon.log:\n{}", log.unwrap_or_default())
    });
    let client = lifecycle.client().clone();

    // A git repo initialized by the real CLI against this daemon.
    let repo = home_path.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let git = Command::new("git")
        .args(["init", "-q"])
        .current_dir(&repo)
        .status()
        .unwrap();
    assert!(git.success());
    let init = Command::new(&pu)
        .arg("init")
        .arg("--socket")
        .arg(&socket)
        .current_dir(&repo)
        .envs(env)
        .output()
        .unwrap();
    assert!(
        init.status.success(),
        "pu init: {}{}",
        String::from_utf8_lossy(&init.stdout),
        String::from_utf8_lossy(&init.stderr)
    );

    let comspec =
        std::env::var("ComSpec").unwrap_or_else(|_| r"C:\Windows\System32\cmd.exe".into());
    let spawn = client
        .request(&Request::Spawn {
            project_root: repo.to_string_lossy().into_owned(),
            prompt: String::new(),
            agent: "terminal".into(),
            name: None,
            base: None,
            root: true,
            worktree: None,
            command: Some(format!("{comspec} /d /q")),
            no_auto: false,
            extra_args: vec![],
            plan_mode: false,
            no_trigger: false,
            trigger: None,
            placement: None,
        })
        .await
        .unwrap();
    let Response::SpawnResult { agent_id, .. } = spawn else {
        panic!("expected SpawnResult, got {spawn:?}");
    };

    let (sink, mut seen) = sink();
    let session =
        AttachSession::start(client.clone(), AttachOptions::new(&agent_id, 100, 30), sink);
    let ready = next_event(&mut seen).await;
    assert!(
        matches!(ready, AttachEvent::AttachReady { .. }),
        "{ready:?}"
    );

    // The typed line holds `%OS%`; only the shell's echo holds the expanded marker.
    session.input(b"echo pp-%OS%-marker\r".to_vec());
    let mut screen = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    while !String::from_utf8_lossy(&screen).contains("pp-Windows_NT-marker") {
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        match timeout(left, seen.recv()).await {
            Ok(Some(Seen::Output(bytes))) => screen.extend(bytes),
            Ok(Some(Seen::Event(_))) => {}
            _ => panic!(
                "marker never echoed; output:\n{}",
                String::from_utf8_lossy(&screen)
            ),
        }
    }

    // Focus claims the pane's size; the owner's resizes are applied and reported back.
    let mut sizes = Vec::new();
    let mut wait_size = async |want: (u16, u16)| loop {
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        match timeout(left, seen.recv()).await {
            Ok(Some(Seen::Event(AttachEvent::PtySize {
                cols, rows, owned, ..
            }))) => {
                sizes.push((cols, rows, owned));
                if (cols, rows) == want {
                    assert!(owned, "{sizes:?}");
                    return;
                }
            }
            Ok(Some(_)) => {}
            _ => panic!("no pty_size {want:?}; saw {sizes:?}"),
        }
    };
    session.claim_size();
    wait_size((100, 30)).await;
    session.resize(90, 25);
    wait_size((90, 25)).await;
    drop(session);

    let _ = client
        .request(&Request::Kill {
            project_root: repo.to_string_lossy().into_owned(),
            target: pu_core::protocol::KillTarget::Agent(agent_id),
            exclude: vec![],
        })
        .await;
    let _ = client.request(&Request::Shutdown).await;
    if let Some(pid) = lifecycle.launched_pid() {
        tokio::task::spawn_blocking(move || {
            crate::daemon::lifecycle::process::wait_exit(pid, Duration::from_secs(10))
        })
        .await
        .unwrap();
    }
}

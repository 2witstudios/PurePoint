//! Subscriptions against a scripted fake daemon (event flow, reconnect, `BUSY`,
//! `daemon_lost`) and against the real `pu-engine.exe` built from this repo.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use pu_core::protocol::GridCommand;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

use super::*;

/// One connection's behaviour: read the subscribe line, write `lines`, then close or hold
/// the connection open (forwarding whatever the client writes).
#[derive(Clone)]
struct Script {
    lines: Vec<String>,
    close: bool,
}

fn hold(lines: &[&str]) -> Script {
    Script {
        lines: lines.iter().map(|l| (*l).to_owned()).collect(),
        close: false,
    }
}

fn close(lines: &[&str]) -> Script {
    Script {
        close: true,
        ..hold(lines)
    }
}

/// A daemon on `socket` answering the n-th connection with `scripts[n]`; connections
/// past the end are closed at once. Every line a client writes arrives on the receiver.
fn fake_daemon(
    socket: &Path,
    scripts: Vec<Script>,
) -> (tokio::task::JoinHandle<()>, mpsc::UnboundedReceiver<String>) {
    let endpoint = pu_transport::Endpoint::for_socket(socket).unwrap();
    let mut listener = pu_transport::Listener::bind(&endpoint).unwrap();
    let (tx, rx) = mpsc::unbounded_channel();
    let task = tokio::spawn(async move {
        let mut scripts = scripts.into_iter();
        let mut connections = Vec::new();
        loop {
            let Ok((stream, _peer)) = listener.accept().await else {
                continue;
            };
            let script = scripts.next().unwrap_or_else(|| close(&[]));
            let tx = tx.clone();
            connections.push(tokio::spawn(async move {
                let (read, mut write) = stream.into_split();
                let mut lines = BufReader::new(read).lines();
                let Ok(Some(first)) = lines.next_line().await else {
                    return;
                };
                let _ = tx.send(first.clone());
                for line in &script.lines {
                    // `AUTO` confirms whichever subscription the client asked for.
                    let line = match line.as_str() {
                        AUTO if first.contains("subscribe_grid") => GRID_SUBSCRIBED,
                        AUTO => STATUS_SUBSCRIBED,
                        other => other,
                    };
                    let _ = write.write_all(format!("{line}\n").as_bytes()).await;
                }
                let _ = write.flush().await;
                if script.close {
                    return;
                }
                while let Ok(Some(line)) = lines.next_line().await {
                    let _ = tx.send(line);
                }
            }));
        }
    });
    (task, rx)
}

fn fast(policy: RetryPolicy) -> RetryPolicy {
    RetryPolicy {
        initial: Duration::from_millis(10),
        max: Duration::from_millis(40),
        ..policy
    }
}

fn fast_policies() -> Policies {
    Policies {
        status: fast(RetryPolicy::STATUS),
        grid: fast(RetryPolicy::GRID),
        projects: fast(RetryPolicy::PROJECTS),
    }
}

fn collector() -> (Emit, mpsc::UnboundedReceiver<SubscriptionEvent>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let emit: Emit = Arc::new(move |event| {
        let _ = tx.send(event);
    });
    (emit, rx)
}

async fn next(rx: &mut mpsc::UnboundedReceiver<SubscriptionEvent>) -> SubscriptionEvent {
    tokio::time::timeout(Duration::from_secs(10), rx.recv())
        .await
        .expect("timed out waiting for an event")
        .expect("emitter dropped")
}

/// The next event's `subscription-state`, panicking on any other event.
async fn next_state(rx: &mut mpsc::UnboundedReceiver<SubscriptionEvent>) -> SubscriptionState {
    match next(rx).await {
        SubscriptionEvent::State(change) => change.state,
        other => panic!("expected a state change, got {other:?}"),
    }
}

/// Events until (and including) the first one matching `want`.
async fn until(
    rx: &mut mpsc::UnboundedReceiver<SubscriptionEvent>,
    want: impl Fn(&SubscriptionEvent) -> bool,
) -> SubscriptionEvent {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let event = rx.recv().await.expect("emitter dropped");
            if want(&event) {
                return event;
            }
        }
    })
    .await
    .expect("timed out waiting for the event")
}

const AUTO: &str = "auto";
const STATUS_SUBSCRIBED: &str = r#"{"type":"status_subscribed"}"#;
const GRID_SUBSCRIBED: &str = r#"{"type":"grid_subscribed"}"#;
const PROJECTS_SUBSCRIBED: &str = r#"{"type":"projects_subscribed"}"#;
const BUSY: &str = r#"{"type":"error","code":"BUSY","message":"too many connections"}"#;
const EMPTY_STATUS: &str = r#"{"type":"status_event","worktrees":[],"agents":[]}"#;

fn grid_event(axis: &str) -> String {
    format!(
        r#"{{"type":"grid_event","project_root":"C:\\p","command":{{"action":"split","leaf_id":null,"axis":"{axis}"}}}}"#
    )
}

fn retrying(state: &SubscriptionState) -> Option<u32> {
    match state {
        SubscriptionState::Retrying { attempt, .. } => Some(*attempt),
        _ => None,
    }
}

#[tokio::test]
async fn given_status_stream_should_emit_subscribed_then_status_events() {
    let tmp = tempfile::TempDir::new().unwrap();
    let socket = tmp.path().join("d.sock");
    let (server, mut requests) = fake_daemon(
        &socket,
        vec![hold(&[STATUS_SUBSCRIBED, "{not json", EMPTY_STATUS])],
    );
    let (emit, mut events) = collector();
    let task = tokio::spawn(status::run(
        DaemonClient::local(&socket).unwrap(),
        "C:\\p".into(),
        fast(RetryPolicy::STATUS),
        emit,
    ));

    assert_eq!(
        requests.recv().await.unwrap(),
        r#"{"type":"subscribe_status","project_root":"C:\\p"}"#
    );
    match next(&mut events).await {
        SubscriptionEvent::State(change) => assert_eq!(
            change,
            StateChange {
                kind: SubscriptionKind::Status,
                project_root: Some("C:\\p".into()),
                state: SubscriptionState::Subscribed
            }
        ),
        other => panic!("{other:?}"),
    }
    // The unreadable line is skipped, as the Swift parse skips unknown replies.
    match next(&mut events).await {
        SubscriptionEvent::Status {
            project_root,
            worktrees,
            agents,
        } => {
            assert_eq!(project_root, "C:\\p");
            assert!(worktrees.is_empty() && agents.is_empty());
        }
        other => panic!("{other:?}"),
    }
    task.abort();
    server.abort();
}

#[tokio::test]
async fn given_busy_daemon_status_should_retry_and_subscribe() {
    let tmp = tempfile::TempDir::new().unwrap();
    let socket = tmp.path().join("d.sock");
    let (server, _requests) = fake_daemon(
        &socket,
        vec![
            close(&[BUSY]),
            close(&[BUSY]),
            hold(&[STATUS_SUBSCRIBED, EMPTY_STATUS]),
        ],
    );
    let (emit, mut events) = collector();
    let task = tokio::spawn(status::run(
        DaemonClient::local(&socket).unwrap(),
        "C:\\p".into(),
        fast(RetryPolicy::STATUS),
        emit,
    ));
    match next_state(&mut events).await {
        SubscriptionState::Retrying {
            attempt, message, ..
        } => {
            assert_eq!(attempt, 1);
            assert!(message.contains("connection limit"), "{message}");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(retrying(&next_state(&mut events).await), Some(2));
    assert_eq!(next_state(&mut events).await, SubscriptionState::Subscribed);
    assert!(matches!(
        next(&mut events).await,
        SubscriptionEvent::Status { .. }
    ));
    task.abort();
    server.abort();
}

#[tokio::test]
async fn given_established_status_stream_dropping_should_report_daemon_lost_once() {
    let tmp = tempfile::TempDir::new().unwrap();
    let socket = tmp.path().join("d.sock");
    let (server, _requests) = fake_daemon(
        &socket,
        vec![
            close(&[STATUS_SUBSCRIBED, EMPTY_STATUS]),
            hold(&[STATUS_SUBSCRIBED]),
        ],
    );
    let (emit, mut events) = collector();
    let task = tokio::spawn(status::run(
        DaemonClient::local(&socket).unwrap(),
        "C:\\p".into(),
        fast(RetryPolicy::STATUS),
        emit,
    ));
    assert_eq!(next_state(&mut events).await, SubscriptionState::Subscribed);
    assert!(matches!(
        next(&mut events).await,
        SubscriptionEvent::Status { .. }
    ));
    // Even though the endpoint answers again, a dropped stream is a lost daemon.
    assert_eq!(next_state(&mut events).await, SubscriptionState::DaemonLost);
    tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .expect("the loop ends after daemon_lost")
        .unwrap();
    assert!(events.try_recv().is_err(), "nothing after daemon_lost");
    server.abort();
}

#[tokio::test]
async fn given_no_daemon_status_should_report_daemon_lost_after_four_attempts() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (emit, mut events) = collector();
    let task = tokio::spawn(status::run(
        DaemonClient::local(&tmp.path().join("none.sock")).unwrap(),
        "C:\\p".into(),
        fast(RetryPolicy::STATUS),
        emit,
    ));
    for attempt in 1..=3 {
        assert_eq!(retrying(&next_state(&mut events).await), Some(attempt));
    }
    assert_eq!(next_state(&mut events).await, SubscriptionState::DaemonLost);
    task.await.unwrap();
}

#[tokio::test]
async fn given_status_subscribe_refused_should_fail_without_retrying() {
    let tmp = tempfile::TempDir::new().unwrap();
    let socket = tmp.path().join("d.sock");
    let refused = r#"{"type":"error","code":"project_root_not_allowed","message":"nope"}"#;
    let (server, _requests) = fake_daemon(&socket, vec![close(&[refused])]);
    let (emit, mut events) = collector();
    status::run(
        DaemonClient::local(&socket).unwrap(),
        "C:\\p".into(),
        fast(RetryPolicy::STATUS),
        emit,
    )
    .await;
    assert_eq!(
        next_state(&mut events).await,
        SubscriptionState::Failed {
            message: "Status subscribe failed: nope".into()
        }
    );
    server.abort();
}

#[tokio::test]
async fn given_grid_stream_dropping_should_reconnect_and_keep_emitting() {
    let tmp = tempfile::TempDir::new().unwrap();
    let socket = tmp.path().join("d.sock");
    let first = grid_event("v");
    let second = grid_event("h");
    let (server, _requests) = fake_daemon(
        &socket,
        vec![
            close(&[GRID_SUBSCRIBED, &first]),
            close(&[BUSY]),
            hold(&[GRID_SUBSCRIBED, &second]),
        ],
    );
    let (emit, mut events) = collector();
    let task = tokio::spawn(grid::run(
        DaemonClient::local(&socket).unwrap(),
        "C:\\p".into(),
        fast(RetryPolicy::GRID),
        emit,
        grid::GridWriter::default(),
    ));
    assert_eq!(next_state(&mut events).await, SubscriptionState::Subscribed);
    let axis = |event: SubscriptionEvent| match event {
        SubscriptionEvent::Grid {
            command: GridCommand::Split { axis, .. },
            ..
        } => axis,
        other => panic!("{other:?}"),
    };
    assert_eq!(axis(next(&mut events).await), "v");
    assert_eq!(retrying(&next_state(&mut events).await), Some(1));
    assert_eq!(retrying(&next_state(&mut events).await), Some(2));
    assert_eq!(next_state(&mut events).await, SubscriptionState::Subscribed);
    assert_eq!(axis(next(&mut events).await), "h");
    task.abort();
    server.abort();
}

#[tokio::test]
async fn given_no_daemon_grid_should_give_up_after_twenty_retries() {
    let tmp = tempfile::TempDir::new().unwrap();
    let (emit, mut events) = collector();
    let policy = RetryPolicy {
        initial: Duration::from_millis(1),
        max: Duration::from_millis(1),
        ..RetryPolicy::GRID
    };
    grid::run(
        DaemonClient::local(&tmp.path().join("none.sock")).unwrap(),
        "C:\\p".into(),
        policy,
        emit,
        grid::GridWriter::default(),
    )
    .await;
    for attempt in 1..=20 {
        assert_eq!(retrying(&next_state(&mut events).await), Some(attempt));
    }
    assert!(matches!(
        next_state(&mut events).await,
        SubscriptionState::Failed { .. }
    ));
}

#[tokio::test]
async fn given_project_subscriptions_should_send_grid_commands_on_the_grid_connection() {
    let tmp = tempfile::TempDir::new().unwrap();
    let socket = tmp.path().join("d.sock");
    let (server, mut requests) = fake_daemon(&socket, vec![hold(&[AUTO]), hold(&[AUTO])]);
    let (emit, mut events) = collector();
    let subscriptions = Subscriptions::new(emit, fast_policies());
    let client = DaemonClient::local(&socket).unwrap();
    // The two connections race, so the fake confirms each by its subscribe line.
    subscriptions.start_project(&client, "C:\\p");
    assert_eq!(subscriptions.project_roots(), vec!["C:\\p".to_owned()]);
    let mut subscribed = Vec::new();
    while subscribed.len() < 2 {
        if let SubscriptionEvent::State(change) = next(&mut events).await {
            assert_eq!(change.state, SubscriptionState::Subscribed);
            subscribed.push(change.kind);
        }
    }
    let mut first = [
        requests.recv().await.unwrap(),
        requests.recv().await.unwrap(),
    ];
    first.sort();
    assert!(first[0].contains("subscribe_grid") && first[1].contains("subscribe_status"));

    let command = GridCommand::Split {
        workspace_id: None,
        leaf_id: Some(1),
        axis: "h".into(),
    };
    assert!(
        subscriptions
            .send_grid_command("C:\\p", command.clone())
            .await
    );
    let sent: serde_json::Value = serde_json::from_str(&requests.recv().await.unwrap()).unwrap();
    assert_eq!(
        sent,
        serde_json::json!({"type": "grid_command", "project_root": "C:\\p",
            "command": {"action": "split", "leaf_id": 1, "axis": "h"}})
    );
    assert!(!subscriptions.send_grid_command("C:\\other", command).await);

    assert!(subscriptions.stop_project("C:\\p"));
    assert!(!subscriptions.stop_project("C:\\p"));
    assert!(subscriptions.project_roots().is_empty());
    server.abort();
}

#[tokio::test]
async fn given_projects_stream_should_resubscribe_after_a_restart() {
    let tmp = tempfile::TempDir::new().unwrap();
    let socket = tmp.path().join("d.sock");
    let (server, mut requests) = fake_daemon(
        &socket,
        vec![
            close(&[
                PROJECTS_SUBSCRIBED,
                r#"{"type":"projects_changed","projects":["C:\\a"]}"#,
            ]),
            hold(&[
                PROJECTS_SUBSCRIBED,
                r#"{"type":"projects_changed","projects":["C:\\a","C:\\b"]}"#,
            ]),
        ],
    );
    let (emit, mut events) = collector();
    let subscriptions = Subscriptions::new(emit, fast_policies());
    let client = DaemonClient::local(&socket).unwrap();
    let (ran_tx, ran_rx) = tokio::sync::oneshot::channel();
    assert!(subscriptions.start_projects(&client, async move {
        let _ = ran_tx.send(());
    }));
    assert!(!subscriptions.start_projects(&client, async {}));
    ran_rx.await.unwrap();
    assert_eq!(
        requests.recv().await.unwrap(),
        r#"{"type":"subscribe_projects"}"#
    );

    let projects = |event: SubscriptionEvent| match event {
        SubscriptionEvent::Projects { projects } => projects,
        other => panic!("{other:?}"),
    };
    assert_eq!(next_state(&mut events).await, SubscriptionState::Subscribed);
    assert_eq!(projects(next(&mut events).await), vec!["C:\\a"]);
    assert_eq!(retrying(&next_state(&mut events).await), Some(1));
    assert_eq!(next_state(&mut events).await, SubscriptionState::Subscribed);
    assert_eq!(projects(next(&mut events).await), vec!["C:\\a", "C:\\b"]);

    assert!(subscriptions.stop_projects());
    assert!(!subscriptions.stop_projects());
    server.abort();
}

#[test]
fn given_events_should_serialize_as_the_front_end_reads_them() {
    let status: SubscriptionEvent = SubscriptionEvent::Status {
        project_root: "C:\\p".into(),
        worktrees: vec![],
        agents: vec![],
    };
    assert_eq!(status.name(), "status-event");
    assert_eq!(
        serde_json::to_value(&status).unwrap(),
        serde_json::json!({"projectRoot": "C:\\p", "worktrees": [], "agents": []})
    );
    let grid = SubscriptionEvent::Grid {
        project_root: "C:\\p".into(),
        command: GridCommand::GetLayout,
    };
    assert_eq!(grid.name(), "grid-event");
    assert_eq!(
        serde_json::to_value(&grid).unwrap(),
        serde_json::json!({"projectRoot": "C:\\p", "command": {"action": "get_layout"}})
    );
    let projects = SubscriptionEvent::Projects {
        projects: vec!["C:\\p".into()],
    };
    assert_eq!(projects.name(), "projects-changed");
    assert_eq!(
        serde_json::to_value(&projects).unwrap(),
        serde_json::json!({"projects": ["C:\\p"]})
    );
    let state = SubscriptionEvent::State(StateChange {
        kind: SubscriptionKind::Status,
        project_root: Some("C:\\p".into()),
        state: SubscriptionState::Retrying {
            attempt: 2,
            delay_ms: 1000,
            message: "m".into(),
        },
    });
    assert_eq!(state.name(), "subscription-state");
    assert_eq!(
        serde_json::to_value(&state).unwrap(),
        serde_json::json!({"kind": "status", "projectRoot": "C:\\p", "state": "retrying",
            "attempt": 2, "delayMs": 1000, "message": "m"})
    );
    let lost = SubscriptionEvent::State(StateChange {
        kind: SubscriptionKind::Projects,
        project_root: None,
        state: SubscriptionState::DaemonLost,
    });
    assert_eq!(
        serde_json::to_value(&lost).unwrap(),
        serde_json::json!({"kind": "projects", "projectRoot": null, "state": "daemon_lost"})
    );
}

#[test]
fn given_swift_timings_should_match_the_policies() {
    let status = RetryPolicy::STATUS;
    assert_eq!(
        (
            status.initial,
            status.max,
            status.max_retries,
            status.daemon_lost_after
        ),
        (
            Duration::from_millis(500),
            Duration::from_secs(5),
            Some(20),
            Some(4)
        )
    );
    // 0.5 + 1 + 2 s of backoff before the fourth attempt fails: about 3.5 s.
    let mut retries = Retries::new(status);
    let mut waited = Duration::ZERO;
    for _ in 0..3 {
        assert!(retries.fail() && !retries.daemon_lost());
        waited += retries.backoff;
        retries.backoff = (retries.backoff * 2).min(status.max);
    }
    assert!(retries.fail() && retries.daemon_lost());
    assert_eq!(waited, Duration::from_millis(3500));
    assert_eq!(RetryPolicy::GRID.daemon_lost_after, None);
    assert_eq!(
        (RetryPolicy::PROJECTS.max, RetryPolicy::PROJECTS.max_retries),
        (Duration::from_secs(10), None)
    );
}

#[cfg(windows)]
mod real_daemon {
    use std::path::PathBuf;
    use std::process::Command;

    use pu_core::protocol::{KillTarget, Request, Response};
    use pu_core::types::AgentStatus;

    use super::*;
    use crate::daemon::lifecycle::{BinaryLocator, DaemonLifecycle};

    /// `target/debug` of the root workspace (`cargo build -p pu-engine -p pu-cli`).
    fn built(exe: &str) -> Option<PathBuf> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../target/debug")
            .join(exe);
        path.is_file()
            .then(|| pu_core::canon::canonicalize(&path).unwrap_or(path))
    }

    fn run(command: &mut Command) {
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{command:?} failed:\n{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn agent_status(event: &SubscriptionEvent, id: &str) -> Option<Option<AgentStatus>> {
        match event {
            SubscriptionEvent::Status { agents, .. } => {
                Some(agents.iter().find(|a| a.id == id).map(|a| a.status))
            }
            _ => None,
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn given_real_daemon_should_push_status_and_grid_events() {
        let (Some(engine), Some(pu)) = (built("pu-engine.exe"), built("pu.exe")) else {
            eprintln!(
                "skipped: build pu-engine and pu first (`cargo build -p pu-engine -p pu-cli` at the repo root)"
            );
            return;
        };
        let home = tempfile::TempDir::new().unwrap();
        let home_path = pu_core::canon::canonicalize(home.path()).unwrap();
        let socket = home_path.join(".pu").join("daemon.sock");
        let env: Vec<(std::ffi::OsString, std::ffi::OsString)> = vec![
            ("HOME".into(), home_path.clone().into()),
            ("USERPROFILE".into(), home_path.clone().into()),
            ("PU_ENGINE_PATH".into(), engine.clone().into()),
        ];
        let lifecycle = DaemonLifecycle::new(
            &socket,
            BinaryLocator {
                dirs: vec![engine.parent().unwrap().to_path_buf()],
                home: None,
                explicit: None,
            },
        )
        .unwrap()
        .with_spawn_env(env.clone());
        let started = lifecycle.ensure_daemon().await;
        let log = || {
            std::fs::read_to_string(home_path.join(".pu").join("daemon.log")).unwrap_or_default()
        };
        started.unwrap_or_else(|e| panic!("{e}; daemon.log:\n{}", log()));
        let pid = lifecycle.launched_pid().unwrap();

        let repo = home_path.join("repo");
        std::fs::create_dir(&repo).unwrap();
        let git = |args: &[&str]| {
            run(Command::new("git")
                .args(args)
                .current_dir(&repo)
                .envs(env.clone()));
        };
        git(&["init", "-q", "-b", "main"]);
        git(&[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "init",
        ]);
        let pu_cmd = |args: &[&str]| {
            run(Command::new(&pu)
                .args(args)
                .current_dir(&repo)
                .envs(env.clone()));
        };
        pu_cmd(&["init"]);
        let root = repo.to_string_lossy().into_owned();

        let (emit, mut events) = collector();
        let subscriptions = Subscriptions::new(emit, fast_policies());
        subscriptions.start_project(lifecycle.client(), &root);
        // The daemon sends a status snapshot on subscribe; the grid stream confirms too.
        let (mut snapshot, mut grid_up) = (false, false);
        while !(snapshot && grid_up) {
            match next(&mut events).await {
                SubscriptionEvent::Status { .. } => snapshot = true,
                SubscriptionEvent::State(StateChange {
                    kind: SubscriptionKind::Grid,
                    state: SubscriptionState::Subscribed,
                    ..
                }) => grid_up = true,
                _ => {}
            }
        }

        let spawned = lifecycle
            .client()
            .request(&Request::Spawn {
                project_root: root.clone(),
                prompt: String::new(),
                agent: "terminal".into(),
                name: None,
                base: None,
                root: true,
                worktree: None,
                command: None,
                no_auto: false,
                extra_args: vec![],
                plan_mode: false,
                no_trigger: true,
                trigger: None,
                placement: None,
            })
            .await
            .unwrap();
        let agent_id = match spawned {
            Response::SpawnResult { agent_id, .. } => agent_id,
            other => panic!("spawn failed: {other:?}; daemon.log:\n{}", log()),
        };
        let listed = until(&mut events, |e| {
            agent_status(e, &agent_id) == Some(Some(AgentStatus::Running))
        })
        .await;
        assert!(
            matches!(listed, SubscriptionEvent::Status { ref project_root, .. } if *project_root == root)
        );

        let killed = lifecycle
            .client()
            .request(&Request::Kill {
                project_root: root.clone(),
                target: KillTarget::Agent(agent_id.clone()),
                exclude: vec![],
            })
            .await
            .unwrap();
        assert!(!matches!(killed, Response::Error { .. }), "{killed:?}");
        until(&mut events, |e| {
            matches!(agent_status(e, &agent_id), Some(status) if status != Some(AgentStatus::Running))
        })
        .await;

        pu_cmd(&["grid", "split", "--axis", "h"]);
        let grid = until(&mut events, |e| matches!(e, SubscriptionEvent::Grid { .. })).await;
        match grid {
            SubscriptionEvent::Grid {
                project_root,
                command: GridCommand::Split { axis, .. },
            } => {
                assert_eq!(project_root, root);
                assert_eq!(axis, "h");
            }
            other => panic!("{other:?}"),
        }

        assert!(subscriptions.stop_project(&root));
        lifecycle
            .client()
            .request(&Request::Shutdown)
            .await
            .unwrap();
        assert!(
            tokio::task::spawn_blocking(move || {
                crate::daemon::lifecycle::process::wait_exit(pid, Duration::from_secs(10))
            })
            .await
            .unwrap(),
            "daemon did not exit after Shutdown"
        );
    }
}

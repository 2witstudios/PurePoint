//! Attach over a real ConPTY session. These drive the engine's
//! attach calls exactly as `ipc_server::streams::run_attachment` does; the
//! named-pipe framing around them is the IPC server's (Windows pipe leaf).

use std::time::Duration;

use super::{AttachmentCursor, Engine};
use crate::pty_manager::{NativePtyHost, SpawnConfig};

const SCRIPT: &str = "Write-Output pu-attach-marker; $l = Read-Host; Write-Output ('got=' + $l); \
    while ($true) { $s = $Host.UI.RawUI.WindowSize; Write-Output ('SIZE=' + $s.Width + 'x' + $s.Height); \
    Start-Sleep -Milliseconds 200 }";

/// A viewer that renders what the attach stream sends it.
struct Viewer {
    cursor: AttachmentCursor,
    parser: vt100::Parser,
    sizes: Vec<(u16, u16, Option<String>)>,
    resyncs: usize,
}

impl Viewer {
    fn screen(&self) -> String {
        self.parser.screen().contents()
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn given_conpty_agent_should_stream_input_marker_and_resize_through_attach() {
    let tmp = tempfile::TempDir::new().unwrap();
    let engine = Engine::new();
    let id = "conpty-attach".to_string();
    let session = NativePtyHost::new()
        .spawn(SpawnConfig {
            command: "pwsh".into(),
            args: vec![
                "-NoProfile".into(),
                "-NoLogo".into(),
                "-Command".into(),
                SCRIPT.into(),
            ],
            cwd: tmp.path().display().to_string(),
            env: vec![],
            env_remove: vec![],
            cols: 80,
            rows: 24,
        })
        .await
        .unwrap();
    engine.sessions.lock().await.insert(id.clone(), session);

    let (buffer, pty, _exit) = engine.get_attach_handles(&id).await.unwrap();
    let (attachment, mut size_rx) = engine.register_attachment(&id, Some("viewer".into())).await;
    let (data, offset, size) = engine
        .attachment_replay(&id, &buffer, true, &mut size_rx)
        .await;
    let mut parser = vt100::Parser::new(size.1, size.0, 0);
    parser.process(&data);
    let mut viewer = Viewer {
        cursor: AttachmentCursor {
            offset,
            size,
            snapshot: true,
            identified: true,
        },
        parser,
        sizes: vec![],
        resyncs: 0,
    };

    async fn pump_until(
        engine: &Engine,
        id: &str,
        buffer: &crate::output_buffer::OutputBuffer,
        size_rx: &mut tokio::sync::broadcast::Receiver<super::PtySize>,
        viewer: &mut Viewer,
        needle: &str,
    ) {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
        while !viewer.screen().contains(needle) {
            assert!(
                tokio::time::Instant::now() < deadline,
                "never saw {needle:?}; screen:\n{}",
                viewer.screen()
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
            let update = engine
                .attachment_update(id, buffer, &mut viewer.cursor, size_rx, None)
                .await;
            if update.resync {
                viewer.resyncs += 1;
                let (cols, rows, _) = viewer.cursor.size.clone();
                viewer.parser = vt100::Parser::new(rows, cols, 0);
            }
            viewer.sizes.extend(update.sizes);
            viewer.parser.process(&update.data);
        }
    }

    // Output streams to the viewer.
    pump_until(
        &engine,
        &id,
        &buffer,
        &mut size_rx,
        &mut viewer,
        "pu-attach-marker",
    )
    .await;

    // Input from the viewer reaches the agent (the attach `Input` path).
    engine
        .write_to_pty(&pty, b"ping-from-attach\r")
        .await
        .unwrap();
    pump_until(
        &engine,
        &id,
        &buffer,
        &mut size_rx,
        &mut viewer,
        "got=ping-from-attach",
    )
    .await;
    pump_until(
        &engine,
        &id,
        &buffer,
        &mut size_rx,
        &mut viewer,
        "SIZE=80x24",
    )
    .await;

    // A size claim resizes the pseudoconsole; the viewer gets the new size
    // and a resync at that geometry, and the agent sees it.
    engine
        .attachment_resize(&id, attachment, 120, 40, true)
        .await;
    assert_eq!(pty.size(), Some((120, 40)));
    assert_eq!(buffer.terminal_size(), (40, 120));
    pump_until(
        &engine,
        &id,
        &buffer,
        &mut size_rx,
        &mut viewer,
        "SIZE=120x40",
    )
    .await;
    assert!(
        viewer
            .sizes
            .contains(&(120, 40, Some("viewer".to_string()))),
        "{:?}",
        viewer.sizes
    );
    assert!(viewer.resyncs >= 1);
    assert_eq!(viewer.parser.screen().size(), (40, 120));

    engine.unregister_attachment(&id, attachment).await;
    // Dropping the session closes the pseudoconsole and the kill-on-close job.
    engine.sessions.lock().await.clear();
}

/// `pu kill` on an agent whose Store-pwsh shell started long-running
/// grandchildren (one attached to the pseudoconsole, one in a hidden window of
/// its own, one under cmd) leaves no process from that tree.
#[tokio::test(flavor = "multi_thread")]
async fn given_agent_with_long_running_grandchildren_should_leave_no_survivors_after_kill() {
    use base64::Engine as _;
    use pu_core::protocol::{KillTarget, Request, Response};

    let tmp = tempfile::TempDir::new().unwrap();
    let project = tmp.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    let pr = project.to_string_lossy().to_string();
    let engine = Engine::new();
    engine
        .handle_request(Request::Init {
            project_root: pr.clone(),
        })
        .await;

    // Absolute paths: the agent PATH the engine builds is not Windows-shaped
    // yet (the engine-env leaf owns it).
    let script = "$p = Join-Path $env:SystemRoot 'System32\\PING.EXE'; \
        Start-Process -NoNewWindow $p -ArgumentList '-t','localhost'; \
        Start-Process $p -ArgumentList '-t','127.0.0.1' -WindowStyle Hidden; \
        & $env:ComSpec /d /c $p -t 127.0.0.1";
    let utf16: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let encoded = base64::engine::general_purpose::STANDARD.encode(utf16);
    let resp = engine
        .handle_request(Request::Spawn {
            project_root: pr.clone(),
            prompt: String::new(),
            agent: "terminal".into(),
            name: None,
            base: None,
            root: true,
            worktree: None,
            command: Some(format!("pwsh -NoProfile -NoLogo -EncodedCommand {encoded}")),
            no_auto: false,
            extra_args: vec![],
            plan_mode: false,
            no_trigger: false,
            trigger: None,
            placement: None,
        })
        .await;
    let agent_id = match resp {
        Response::SpawnResult { agent_id, .. } => agent_id,
        other => panic!("expected SpawnResult, got {other:?}"),
    };
    let root_pid = engine.sessions.lock().await[&agent_id].pid;

    // Wait for the whole tree: three ping.exe under pwsh (one via cmd).
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    let tree = loop {
        let tree = crate::pty_manager::descendant_probes(root_pid);
        let pings = tree
            .iter()
            .filter(|p| p.name.eq_ignore_ascii_case("ping.exe"))
            .count();
        if pings >= 3 {
            break tree;
        }
        if tokio::time::Instant::now() >= deadline {
            let output = engine.sessions.lock().await[&agent_id]
                .output_buffer
                .read_all();
            panic!(
                "grandchildren never started: {:?}
{}",
                tree.iter().map(|p| (&p.name, p.pid)).collect::<Vec<_>>(),
                String::from_utf8_lossy(&output)
            );
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    };

    let started = std::time::Instant::now();
    let resp = engine
        .handle_request(Request::Kill {
            project_root: pr,
            target: KillTarget::Agent(agent_id.clone()),
            exclude: vec![],
        })
        .await;
    assert!(matches!(resp, Response::KillResult { .. }), "{resp:?}");
    // kill_agents uses a 5 s grace; the stop must finish within grace + 1 s.
    assert!(
        started.elapsed() < Duration::from_secs(6),
        "{:?}",
        started.elapsed()
    );
    assert!(!engine.sessions.lock().await.contains_key(&agent_id));

    // TerminateJobObject/TerminateProcess complete asynchronously.
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while tree.iter().any(|p| p.is_alive()) && std::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let survivors: Vec<_> = tree
        .iter()
        .filter(|p| p.is_alive())
        .map(|p| (&p.name, p.pid))
        .collect();
    assert!(survivors.is_empty(), "survived kill: {survivors:?}");
}

/// `kill_all_sessions` (shutdown) and `Engine::drop` stop the tree too.
#[tokio::test(flavor = "multi_thread")]
async fn given_shutdown_or_drop_should_end_every_agent_tree() {
    let tmp = tempfile::TempDir::new().unwrap();
    let spawn = || async {
        NativePtyHost::new()
            .spawn(SpawnConfig {
                command: "cmd".into(),
                args: vec!["/d".into(), "/c".into(), "ping -t 127.0.0.1".into()],
                cwd: tmp.path().display().to_string(),
                env: vec![],
                env_remove: vec![],
                cols: 80,
                rows: 24,
            })
            .await
            .unwrap()
    };
    async fn ping_of(pid: u32) -> crate::pty_manager::DescendantProbe {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(p) = crate::pty_manager::descendant_probes(pid)
                .into_iter()
                .find(|p| p.name.eq_ignore_ascii_case("ping.exe"))
            {
                return p;
            }
            assert!(tokio::time::Instant::now() < deadline, "ping never started");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
    async fn gone(p: &crate::pty_manager::DescendantProbe) -> bool {
        for _ in 0..40 {
            if !p.is_alive() {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        false
    }

    // Shutdown: marked killed by shutdown, tree gone.
    let engine = Engine::new();
    let handle = spawn().await;
    let (pid, mut exit) = (handle.pid, handle.exit_rx.clone());
    engine.sessions.lock().await.insert("a".into(), handle);
    let ping = ping_of(pid).await;
    engine.kill_all_sessions(Duration::from_millis(200)).await;
    assert!(gone(&ping).await, "ping survived kill_all_sessions");
    assert!(engine.shutdown_killed.lock().unwrap().contains(&pid));
    let code = tokio::time::timeout(Duration::from_secs(2), async {
        while exit.borrow_and_update().is_none() {
            exit.changed().await.unwrap();
        }
        *exit.borrow()
    })
    .await
    .unwrap();
    assert!(code.is_some());

    // Drop: a hard job kill, reported as 137.
    let engine = Engine::new();
    let handle = spawn().await;
    let (pid, mut exit) = (handle.pid, handle.exit_rx.clone());
    engine.sessions.lock().await.insert("b".into(), handle);
    let ping = ping_of(pid).await;
    drop(engine);
    assert!(gone(&ping).await, "ping survived Engine::drop");
    let code = tokio::time::timeout(Duration::from_secs(3), async {
        while exit.borrow_and_update().is_none() {
            exit.changed().await.unwrap();
        }
        *exit.borrow()
    })
    .await
    .unwrap();
    assert_eq!(code, Some(137));
}

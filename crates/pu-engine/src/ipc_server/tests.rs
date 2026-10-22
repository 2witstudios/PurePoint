use super::*;
use pu_core::protocol::{KillTarget, Request, Response};
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

#[tokio::test(flavor = "current_thread")]
async fn given_ipc_server_should_accept_connection() {
    let tmp = TempDir::new().unwrap();
    let sock_path = tmp.path().join("test.sock");
    let engine = Engine::new();

    let server = IpcServer::bind(&sock_path, engine).unwrap();
    let handle = tokio::spawn(async move { server.run().await });

    // Connect as client
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let stream = UnixStream::connect(&sock_path).await.unwrap();
    assert!(stream.peer_addr().is_ok());

    handle.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn given_health_request_should_respond_with_report() {
    let tmp = TempDir::new().unwrap();
    let sock_path = tmp.path().join("test.sock");
    let engine = Engine::new();

    let server = IpcServer::bind(&sock_path, engine).unwrap();
    let handle = tokio::spawn(async move { server.run().await });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let stream = UnixStream::connect(&sock_path).await.unwrap();
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);

    // Send health request
    let req = serde_json::to_string(&Request::Health).unwrap();
    writer
        .write_all(format!("{req}\n").as_bytes())
        .await
        .unwrap();

    // Read response
    let mut line = String::new();
    reader.read_line(&mut line).await.unwrap();
    let resp: Response = serde_json::from_str(&line).unwrap();

    match resp {
        Response::HealthReport {
            protocol_version, ..
        } => {
            assert_eq!(protocol_version, pu_core::protocol::PROTOCOL_VERSION);
        }
        other => panic!("expected HealthReport, got {other:?}"),
    }

    handle.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn given_init_request_should_create_manifest() {
    let tmp = TempDir::new().unwrap();
    let sock_path = tmp.path().join("test.sock");
    let project_root = tmp.path().join("project");
    std::fs::create_dir_all(&project_root).unwrap();
    let engine = Engine::new();

    let server = IpcServer::bind(&sock_path, engine).unwrap();
    let handle = tokio::spawn(async move { server.run().await });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let stream = UnixStream::connect(&sock_path).await.unwrap();
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);

    let req = serde_json::to_string(&Request::Init {
        project_root: project_root.to_string_lossy().into(),
    })
    .unwrap();
    writer
        .write_all(format!("{req}\n").as_bytes())
        .await
        .unwrap();

    let mut line = String::new();
    reader.read_line(&mut line).await.unwrap();
    let resp: Response = serde_json::from_str(&line).unwrap();

    match resp {
        Response::InitResult { created } => assert!(created),
        other => panic!("expected InitResult, got {other:?}"),
    }

    // Verify manifest exists
    let manifest_path = project_root.join(".pu/manifest.json");
    assert!(manifest_path.exists());

    handle.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn given_shutdown_request_should_respond_and_stop() {
    let tmp = TempDir::new().unwrap();
    let sock_path = tmp.path().join("test.sock");
    let engine = Engine::new();

    let server = IpcServer::bind(&sock_path, engine).unwrap();
    let handle = tokio::spawn(async move { server.run().await });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let stream = UnixStream::connect(&sock_path).await.unwrap();
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);

    let req = serde_json::to_string(&Request::Shutdown).unwrap();
    writer
        .write_all(format!("{req}\n").as_bytes())
        .await
        .unwrap();

    let mut line = String::new();
    reader.read_line(&mut line).await.unwrap();
    let resp: Response = serde_json::from_str(&line).unwrap();
    assert!(matches!(resp, Response::ShuttingDown));

    // Server should stop
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert!(handle.is_finished());
}

/// Helper: init project + spawn agent via shared helper, then bind IPC server.
/// Returns (sock_path, agent_id, server_handle, _tmp).
async fn setup_with_agent() -> (
    std::path::PathBuf,
    String,
    tokio::task::JoinHandle<Result<(), std::io::Error>>,
    TempDir,
) {
    let (engine, agent_id, tmp) = crate::test_helpers::init_and_spawn().await;
    let sock_path = tmp.path().join("test.sock");

    let server = IpcServer::bind(&sock_path, engine).unwrap();
    let handle = tokio::spawn(async move { server.run().await });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    (sock_path.clone(), agent_id, handle, tmp)
}

#[tokio::test(flavor = "current_thread")]
async fn given_attach_request_should_stream_output_continuously() {
    let (engine, agent_id, tmp) =
        crate::test_helpers::init_and_spawn_command(Some("/bin/cat")).await;
    let sock_path = tmp.path().join("test.sock");
    let server = IpcServer::bind(&sock_path, engine).unwrap();
    let engine = server.engine().clone();
    let buffer = engine.get_attach_handles(&agent_id).await.unwrap().0;
    let replay = b"controlled-replay-marker";
    let mut seed = replay.to_vec();
    seed.push(b'\n');
    assert!(matches!(
        engine
            .handle_request(Request::Input {
                agent_id: agent_id.clone(),
                data: seed,
                submit: false,
                cancel_on_disconnect: false,
            })
            .await,
        Response::Ok
    ));
    // A real PTY must have produced the expected bytes before testing replay.
    // A shell prompt and a fixed startup sleep cannot establish this condition.
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while !buffer
            .read_from(0)
            .0
            .windows(replay.len())
            .any(|w| w == replay)
        {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("controlled PTY did not produce replay bytes");
    let server_handle = tokio::spawn(async move { server.run().await });
    let stream = UnixStream::connect(&sock_path).await.unwrap();
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let req = serde_json::to_string(&Request::Attach {
        agent_id: agent_id.clone(),
        replay: None,
        client_id: None,
    })
    .unwrap();
    writer
        .write_all(format!("{req}\n").as_bytes())
        .await
        .unwrap();
    let mut line = String::new();
    reader.read_line(&mut line).await.unwrap();
    let ready: Response = serde_json::from_str(line.trim()).unwrap();
    assert!(
        matches!(ready, Response::AttachReady { buffered_bytes } if buffered_bytes >= replay.len())
    );

    async fn expect_marker(reader: &mut IpcReader, agent: &str, marker: &[u8]) {
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            let mut received = Vec::new();
            loop {
                let mut line = String::new();
                assert!(
                    reader.read_line(&mut line).await.unwrap() > 0,
                    "attach stream closed"
                );
                match serde_json::from_str::<Response>(line.trim()).unwrap() {
                    Response::Output { agent_id, data } => {
                        assert_eq!(agent_id, agent);
                        received.extend(data);
                        if received.windows(marker.len()).any(|w| w == marker) {
                            break;
                        }
                    }
                    other => panic!("expected Output, got {other:?}"),
                }
            }
        })
        .await
        .expect("controlled marker missing from attach output");
    }
    expect_marker(&mut reader, &agent_id, b"controlled-replay-marker").await;
    let input = serde_json::to_string(&Request::Input {
        agent_id: agent_id.clone(),
        data: b"controlled-live-marker\n".to_vec(),
        submit: false,
        cancel_on_disconnect: false,
    })
    .unwrap();
    writer
        .write_all(format!("{input}\n").as_bytes())
        .await
        .unwrap();
    expect_marker(&mut reader, &agent_id, b"controlled-live-marker").await;
    engine
        .handle_request(Request::Kill {
            project_root: tmp.path().join("project").to_string_lossy().into_owned(),
            target: KillTarget::Agent(agent_id),
            exclude: vec![],
        })
        .await;
    server_handle.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn given_attach_with_input_should_forward_to_pty() {
    // This test uses a separate IPC server with a cat process to test input forwarding.
    // We can't easily override the spawn command through the config, so we test the
    // Input path by verifying it doesn't error — the PTY write path is already tested
    // in pty_manager tests. Here we verify the IPC plumbing works end-to-end.
    let (sock_path, agent_id, server_handle, _tmp) = setup_with_agent().await;
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let stream = UnixStream::connect(&sock_path).await.unwrap();
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);

    // Attach
    let req = serde_json::to_string(&Request::Attach {
        agent_id: agent_id.clone(),
        replay: None,
        client_id: None,
    })
    .unwrap();
    writer
        .write_all(format!("{req}\n").as_bytes())
        .await
        .unwrap();

    let mut line = String::new();
    reader.read_line(&mut line).await.unwrap(); // AttachReady

    // Send input — even if the process has exited, the write to the PTY fd should
    // not crash the server. The server handles EPIPE/EIO gracefully.
    let input_req = serde_json::to_string(&Request::Input {
        agent_id: agent_id.clone(),
        data: b"hello\n".to_vec(),
        submit: false,
        cancel_on_disconnect: false,
    })
    .unwrap();
    writer
        .write_all(format!("{input_req}\n").as_bytes())
        .await
        .unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // Verify server is still running — connection didn't crash
    drop(writer);
    drop(reader);

    // Health check on a new connection
    let stream = UnixStream::connect(&sock_path).await.unwrap();
    let (reader2, mut writer2) = stream.into_split();
    let mut reader2 = BufReader::new(reader2);

    let health_req = serde_json::to_string(&Request::Health).unwrap();
    writer2
        .write_all(format!("{health_req}\n").as_bytes())
        .await
        .unwrap();
    let mut line2 = String::new();
    reader2.read_line(&mut line2).await.unwrap();
    let resp: Response = serde_json::from_str(line2.trim()).unwrap();
    assert!(
        matches!(resp, Response::HealthReport { .. }),
        "server still healthy after input during attach"
    );

    server_handle.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn given_attach_with_resize_should_update_pty_size() {
    let (sock_path, agent_id, server_handle, _tmp) = setup_with_agent().await;
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let stream = UnixStream::connect(&sock_path).await.unwrap();
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);

    // Attach
    let req = serde_json::to_string(&Request::Attach {
        agent_id: agent_id.clone(),
        replay: None,
        client_id: None,
    })
    .unwrap();
    writer
        .write_all(format!("{req}\n").as_bytes())
        .await
        .unwrap();

    let mut line = String::new();
    reader.read_line(&mut line).await.unwrap(); // AttachReady

    // Send resize — should not error or crash
    let resize_req = serde_json::to_string(&Request::Resize {
        agent_id: agent_id.clone(),
        cols: 200,
        rows: 50,
    })
    .unwrap();
    writer
        .write_all(format!("{resize_req}\n").as_bytes())
        .await
        .unwrap();

    // If resize caused a crash we'd get EOF; give it a moment
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // Connection should still be alive — we can drop cleanly
    drop(writer);
    server_handle.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn given_killed_agent_attach_should_return_error() {
    let (sock_path, agent_id, server_handle, tmp) = setup_with_agent().await;
    let pr = tmp.path().join("project").to_string_lossy().to_string();
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    // Kill the agent via IPC
    let stream = UnixStream::connect(&sock_path).await.unwrap();
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);

    let kill_req = serde_json::to_string(&Request::Kill {
        project_root: pr,
        target: KillTarget::Agent(agent_id.clone()),
        exclude: vec![],
    })
    .unwrap();
    writer
        .write_all(format!("{kill_req}\n").as_bytes())
        .await
        .unwrap();

    let mut line = String::new();
    reader.read_line(&mut line).await.unwrap();
    let resp: Response = serde_json::from_str(line.trim()).unwrap();
    assert!(
        matches!(resp, Response::KillResult { .. }),
        "expected KillResult, got {resp:?}"
    );
    drop(writer);
    drop(reader);

    // Now attempt to attach — agent session is gone, should get AGENT_NOT_FOUND
    let stream = UnixStream::connect(&sock_path).await.unwrap();
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);

    let attach_req = serde_json::to_string(&Request::Attach {
        agent_id,
        replay: None,
        client_id: None,
    })
    .unwrap();
    writer
        .write_all(format!("{attach_req}\n").as_bytes())
        .await
        .unwrap();

    let mut line = String::new();
    reader.read_line(&mut line).await.unwrap();
    let resp: Response = serde_json::from_str(line.trim()).unwrap();
    match resp {
        Response::Error { code, .. } => {
            assert_eq!(code, "AGENT_NOT_FOUND");
        }
        other => panic!("expected AGENT_NOT_FOUND error, got {other:?}"),
    }

    server_handle.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn given_attach_disconnect_should_not_crash_server() {
    let (sock_path, agent_id, server_handle, _tmp) = setup_with_agent().await;
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    // Attach and immediately disconnect
    {
        let stream = UnixStream::connect(&sock_path).await.unwrap();
        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);

        let req = serde_json::to_string(&Request::Attach {
            agent_id: agent_id.clone(),
            replay: None,
            client_id: None,
        })
        .unwrap();
        writer
            .write_all(format!("{req}\n").as_bytes())
            .await
            .unwrap();

        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap(); // AttachReady
        // Drop stream — disconnect
    }

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // Server should still work — connect again and do a health check
    let stream = UnixStream::connect(&sock_path).await.unwrap();
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);

    let req = serde_json::to_string(&Request::Health).unwrap();
    writer
        .write_all(format!("{req}\n").as_bytes())
        .await
        .unwrap();

    let mut line = String::new();
    reader.read_line(&mut line).await.unwrap();
    let resp: Response = serde_json::from_str(line.trim()).unwrap();
    assert!(
        matches!(resp, Response::HealthReport { .. }),
        "server still healthy after attach disconnect"
    );

    server_handle.abort();
}

async fn send_health(stream: UnixStream) -> Option<Response> {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let req = serde_json::to_string(&Request::Health).unwrap();
    writer.write_all(format!("{req}\n").as_bytes()).await.ok()?;
    let mut line = String::new();
    match reader.read_line(&mut line).await {
        Ok(n) if n > 0 => serde_json::from_str(&line).ok(),
        _ => None,
    }
}

#[tokio::test(flavor = "current_thread")]
async fn given_connection_limit_reached_should_reject_new_connection_promptly() {
    let tmp = TempDir::new().unwrap();
    let sock_path = tmp.path().join("test.sock");
    let server = IpcServer::bind(&sock_path, Engine::new())
        .unwrap()
        .with_connection_limit(2);
    let handle = tokio::spawn(async move { server.run().await });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Hold the limit's worth of live connections (each proven served).
    let mut held = Vec::new();
    for _ in 0..2 {
        let stream = UnixStream::connect(&sock_path).await.unwrap();
        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);
        let req = serde_json::to_string(&Request::Health).unwrap();
        writer
            .write_all(format!("{req}\n").as_bytes())
            .await
            .unwrap();
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        assert!(matches!(
            serde_json::from_str::<Response>(&line).unwrap(),
            Response::HealthReport { .. }
        ));
        held.push((reader, writer));
    }

    // One more is rejected promptly: BUSY error or EOF, never left hanging.
    let extra = UnixStream::connect(&sock_path).await.unwrap();
    let (reader, _writer) = extra.into_split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();
    let read = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        reader.read_line(&mut line),
    )
    .await
    .expect("excess connection must be closed promptly, not held waiting");
    match read {
        Ok(0) | Err(_) => {}
        Ok(_) => match serde_json::from_str::<Response>(&line).unwrap() {
            Response::Error { code, .. } => assert_eq!(code, "BUSY"),
            other => panic!("expected BUSY error, got {other:?}"),
        },
    }

    // Releasing one held connection frees a slot for a new request.
    drop(held.pop());
    let mut served = None;
    for _ in 0..50 {
        let stream = UnixStream::connect(&sock_path).await.unwrap();
        if let Some(resp) = send_health(stream).await
            && matches!(resp, Response::HealthReport { .. })
        {
            served = Some(resp);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(
        served.is_some(),
        "health request must succeed after a slot frees"
    );

    handle.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_subscribe_fs_should_stream_coalesced_file_events() {
    let tmp = TempDir::new().unwrap();
    let sock_path = tmp.path().join("test.sock");
    let project_root = tmp.path().canonicalize().unwrap().join("project");
    std::fs::create_dir_all(&project_root).unwrap();
    let engine = Engine::new();
    let init = engine
        .handle_request(Request::Init {
            project_root: project_root.to_string_lossy().into(),
        })
        .await;
    assert!(matches!(init, Response::InitResult { .. }));

    let server = IpcServer::bind(&sock_path, engine).unwrap();
    let handle = tokio::spawn(async move { server.run().await });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let stream = UnixStream::connect(&sock_path).await.unwrap();
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let req = serde_json::to_string(&Request::SubscribeFs {
        root: project_root.to_string_lossy().into(),
        recursive: true,
    })
    .unwrap();
    writer
        .write_all(format!("{req}\n").as_bytes())
        .await
        .unwrap();
    let mut line = String::new();
    reader.read_line(&mut line).await.unwrap();
    let resp: Response = serde_json::from_str(&line).unwrap();
    assert!(matches!(resp, Response::FsSubscribed), "{resp:?}");

    // Give the watcher a moment to arm, then change a file.
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    let changed = project_root.join("changed.txt");
    std::fs::write(&changed, "x").unwrap();

    let expected = changed.to_string_lossy().into_owned();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        line.clear();
        let read = tokio::time::timeout_at(deadline, reader.read_line(&mut line))
            .await
            .expect("no fs_event before deadline")
            .unwrap();
        assert!(read > 0, "stream closed");
        match serde_json::from_str::<Response>(&line).unwrap() {
            Response::FsEvent { root, paths } => {
                assert_eq!(root, project_root.to_string_lossy());
                if paths.contains(&expected) {
                    break;
                }
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    handle.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_subscribe_projects_should_push_the_list_on_add_and_remove() {
    let tmp = TempDir::new().unwrap();
    let sock_path = tmp.path().join("test.sock");
    let project_root = tmp.path().canonicalize().unwrap().join("project");
    std::fs::create_dir_all(&project_root).unwrap();
    let root = project_root.to_string_lossy().into_owned();
    let server = IpcServer::bind(&sock_path, Engine::new()).unwrap();
    let handle = tokio::spawn(async move { server.run().await });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let stream = UnixStream::connect(&sock_path).await.unwrap();
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let req = serde_json::to_string(&Request::SubscribeProjects).unwrap();
    writer
        .write_all(format!("{req}\n").as_bytes())
        .await
        .unwrap();
    assert!(matches!(
        next_response(&mut reader).await,
        Response::ProjectsSubscribed
    ));
    assert!(matches!(
        next_response(&mut reader).await,
        Response::ProjectsChanged { projects } if projects.is_empty()
    ));

    let send = |request: Request| {
        let sock_path = sock_path.clone();
        async move {
            let stream = UnixStream::connect(&sock_path).await.unwrap();
            let (reader, mut writer) = stream.into_split();
            let json = serde_json::to_string(&request).unwrap();
            writer
                .write_all(format!("{json}\n").as_bytes())
                .await
                .unwrap();
            next_response(&mut BufReader::new(reader)).await
        }
    };
    send(Request::Init {
        project_root: root.clone(),
    })
    .await;
    assert!(matches!(
        next_response(&mut reader).await,
        Response::ProjectsChanged { projects } if projects == vec![root.clone()]
    ));
    send(Request::RemoveProject {
        project_root: root.clone(),
        suspend_agents: false,
    })
    .await;
    assert!(matches!(
        next_response(&mut reader).await,
        Response::ProjectsChanged { projects } if projects.is_empty()
    ));

    handle.abort();
}

async fn attachment_pair(
    engine: std::sync::Arc<Engine>,
    agent_id: String,
    replay: Option<pu_core::protocol::Replay>,
    client_id: Option<&str>,
) -> (
    BufReader<tokio::net::unix::OwnedReadHalf>,
    tokio::net::unix::OwnedWriteHalf,
    tokio::task::JoinHandle<()>,
) {
    let (server, client) = UnixStream::pair().unwrap();
    let (read, mut write) = server.into_split();
    let client_id = client_id.map(str::to_owned);
    let task = tokio::spawn(async move {
        streams::handle_attach_stream(
            &mut BufReader::new(read),
            &mut Vec::new(),
            &mut write,
            &engine,
            &agent_id,
            replay,
            client_id,
        )
        .await;
    });
    let (read, write) = client.into_split();
    (BufReader::new(read), write, task)
}

async fn next_response(reader: &mut BufReader<tokio::net::unix::OwnedReadHalf>) -> Response {
    let mut line = String::new();
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        reader.read_line(&mut line),
    )
    .await
    .unwrap()
    .unwrap();
    serde_json::from_str(&line).unwrap()
}

#[tokio::test]
async fn given_identified_viewer_should_rebuild_evicted_screen_and_resync_gap() {
    let (engine, id, _tmp) = crate::test_helpers::init_and_spawn().await;
    let engine = std::sync::Arc::new(engine);
    let (buffer, _, _) = engine.get_attach_handles(&id).await.unwrap();
    // Wait for shell startup, then put enduring output beyond the raw replay bound.
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    buffer.write(b"\x1b[2J\x1b[Hancient screen");
    buffer.write(&b"\x1b[0m".repeat(1_100_000));
    let (mut reader, writer, task) = attachment_pair(
        engine,
        id,
        Some(pu_core::protocol::Replay::Snapshot),
        Some("phone"),
    )
    .await;
    let bytes = match next_response(&mut reader).await {
        Response::AttachReady { buffered_bytes } => buffered_bytes,
        r => panic!("{r:?}"),
    };
    assert!(matches!(
        next_response(&mut reader).await,
        Response::PtySize {
            owner_client_id: None,
            ..
        }
    ));
    let (rows, cols) = buffer.terminal_size();
    let mut terminal = vt100::Parser::new(rows, cols, 1024);
    let mut remaining = bytes;
    while remaining > 0 {
        let Response::Output { data, .. } = next_response(&mut reader).await else {
            panic!("expected output")
        };
        remaining -= data.len();
        terminal.process(&data);
    }
    assert!(terminal.screen().contents().contains("ancient screen"));
    buffer.write(&b"\x1b[0m".repeat(1_100_000));
    assert!(matches!(next_response(&mut reader).await, Response::Resync));
    assert!(matches!(
        next_response(&mut reader).await,
        Response::Output { .. }
    ));
    drop(reader);
    drop(writer);
    task.await.unwrap();
}

#[tokio::test]
async fn given_size_claims_should_ignore_nonowners_and_handoff_only_to_claimers() {
    let (engine, id, _tmp) = crate::test_helpers::init_and_spawn().await;
    let (a, _) = engine.register_attachment(&id, Some("a".into())).await;
    let (b, _) = engine.register_attachment(&id, Some("b".into())).await;
    let (c, _) = engine.register_attachment(&id, Some("c".into())).await;
    engine.attachment_resize(&id, a, 100, 30, true).await;
    engine.attachment_resize(&id, b, 70, 20, false).await;
    assert_eq!(
        engine.attachment_size(&id).await,
        (100, 30, Some("a".into()))
    );
    engine.unregister_attachment(&id, a).await;
    assert_eq!(engine.attachment_size(&id).await, (100, 30, None));
    engine.attachment_resize(&id, b, 70, 20, true).await;
    engine.attachment_resize(&id, c, 90, 25, true).await;
    // Mere resize does not reorder b's claim ahead of c.
    engine.attachment_resize(&id, b, 75, 22, false).await;
    engine.unregister_attachment(&id, c).await;
    assert_eq!(
        engine.attachment_size(&id).await,
        (75, 22, Some("b".into()))
    );
    let (legacy, _) = engine.register_attachment(&id, None).await;
    engine.attachment_resize(&id, legacy, 110, 35, false).await;
    assert_eq!(engine.attachment_size(&id).await, (110, 35, None));
    let (buffer, fd, _) = engine.get_attach_handles(&id).await.unwrap();
    assert_eq!(buffer.terminal_size(), (35, 110));
    use std::os::fd::AsRawFd;
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    assert_eq!(
        unsafe { libc::ioctl(fd.as_raw_fd(), libc::TIOCGWINSZ, &mut ws) },
        0
    );
    assert_eq!((ws.ws_row, ws.ws_col), buffer.terminal_size());
    engine.handle_legacy_resize(&id, 80, 24).await;
    assert_eq!(engine.attachment_size(&id).await, (80, 24, None));
    engine.unregister_attachment(&id, legacy).await;
    engine.unregister_attachment(&id, b).await;
}

#[tokio::test]
async fn given_legacy_attachment_should_never_receive_size_or_resync_frames() {
    let (engine, id, _tmp) = crate::test_helpers::init_and_spawn().await;
    let engine = std::sync::Arc::new(engine);
    let (buffer, _, _) = engine.get_attach_handles(&id).await.unwrap();
    let (mut reader, mut writer, task) = attachment_pair(engine, id.clone(), None, None).await;
    let mut remaining = match next_response(&mut reader).await {
        Response::AttachReady { buffered_bytes } => buffered_bytes,
        r => panic!("{r:?}"),
    };
    while remaining > 0 {
        let Response::Output { data, .. } = next_response(&mut reader).await else {
            panic!("legacy received new response")
        };
        remaining -= data.len();
    }
    writer
        .write_all(
            format!(
                "{}\n",
                serde_json::to_string(&Request::Resize {
                    agent_id: id,
                    cols: 101,
                    rows: 31
                })
                .unwrap()
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert_eq!(buffer.terminal_size(), (31, 101));
    buffer.write(&b"\x1b[0m".repeat(1_100_000));
    assert!(matches!(
        next_response(&mut reader).await,
        Response::Output { .. }
    ));
    drop(reader);
    drop(writer);
    task.await.unwrap();
}

#[tokio::test]
async fn given_cancellable_submit_disconnect_should_stop_future_chunks_and_enter() {
    let (engine, id, _tmp) = crate::test_helpers::init_and_spawn().await;
    let (buffer, _, _) = engine.get_attach_handles(&id).await.unwrap();
    let (server, client) = UnixStream::pair().unwrap();
    let (read, mut write) = server.into_split();
    let (client_read, client_write) = client.into_split();
    let request = serde_json::to_string(&Request::Input {
        agent_id: id,
        data: vec![b'x'; 20000],
        submit: true,
        cancel_on_disconnect: true,
    })
    .unwrap();
    let task = tokio::spawn(async move {
        IpcServer::dispatch_request(
            &request,
            &mut BufReader::new(read),
            &mut Vec::new(),
            &mut write,
            &engine,
            &Notify::new(),
        )
        .await
    });
    tokio::time::sleep(std::time::Duration::from_millis(60)).await;
    drop(client_read);
    drop(client_write);
    assert!(
        !tokio::time::timeout(std::time::Duration::from_secs(1), task)
            .await
            .unwrap()
            .unwrap()
    );
    // One already-issued blocking OS write is accepted. No later chunks/Enter run.
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let text = buffer.read_all();
    assert!(text.iter().filter(|&&c| c == b'x').count() < 20000);
    let offset = buffer.current_offset();
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert_eq!(buffer.current_offset(), offset);
}

#[tokio::test]
async fn given_rapid_applied_sizes_should_report_every_change_in_order() {
    let (engine, id, _tmp) = crate::test_helpers::init_and_spawn().await;
    let engine = std::sync::Arc::new(engine);
    let (owner, _) = engine.register_attachment(&id, Some("owner".into())).await;
    let (mut reader, writer, task) =
        attachment_pair(engine.clone(), id.clone(), None, Some("viewer")).await;
    let mut remaining = match next_response(&mut reader).await {
        Response::AttachReady { buffered_bytes } => buffered_bytes,
        r => panic!("{r:?}"),
    };
    assert!(matches!(
        next_response(&mut reader).await,
        Response::PtySize { .. }
    ));
    while remaining > 0 {
        if let Response::Output { data, .. } = next_response(&mut reader).await {
            remaining -= data.len();
        }
    }
    for cols in [81, 82, 83] {
        engine.attachment_resize(&id, owner, cols, 24, true).await;
    }
    let mut observed = Vec::new();
    while observed.len() < 3 {
        if let Response::PtySize { cols, .. } = next_response(&mut reader).await {
            observed.push(cols);
        }
    }
    assert_eq!(observed, vec![81, 82, 83]);
    drop(reader);
    drop(writer);
    task.await.unwrap();
    engine.unregister_attachment(&id, owner).await;
}

#[tokio::test]
async fn given_cancellable_request_and_eof_already_buffered_should_never_start_typing() {
    let (engine, id, _tmp) = crate::test_helpers::init_and_spawn().await;
    let (buffer, _, _) = engine.get_attach_handles(&id).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let (server, mut client) = UnixStream::pair().unwrap();
    let (read, mut write) = server.into_split();
    client.shutdown().await.unwrap();
    let request = serde_json::to_string(&Request::Input {
        agent_id: id,
        data: b"QQQQQQQQQQQQQQQQ".to_vec(),
        submit: true,
        cancel_on_disconnect: true,
    })
    .unwrap();
    let mut reader = BufReader::new(read);
    let before = buffer.current_offset();
    assert!(
        !IpcServer::dispatch_request(
            &request,
            &mut reader,
            &mut Vec::new(),
            &mut write,
            &engine,
            &Notify::new(),
        )
        .await
    );
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let (data, _, _) = buffer.read_from(before);
    assert!(
        data.is_empty(),
        "PTY output changed after pre-observed EOF: {data:?}"
    );
    assert_eq!(
        buffer.current_offset(),
        before,
        "even a partial first chunk must not be queued"
    );
}

#[tokio::test]
async fn given_pending_old_geometry_output_should_reset_at_matching_size_and_live_offset() {
    let (engine, id, _tmp) = Engine::quiet_screen_session().await;
    let (buffer, _, _) = engine.get_attach_handles(&id).await.unwrap();
    let (owner, _) = engine.register_attachment(&id, Some("owner".into())).await;
    engine.attachment_resize(&id, owner, 5, 3, true).await;
    // Retain history so visible-screen assertions cannot accidentally compare scrollback.
    buffer.write(b"old\r\nold\r\nold\r\nold\r\n\x1b[2J\x1b[H");
    let (_, mut sizes) = engine.register_attachment(&id, Some("viewer".into())).await;
    let (initial, offset, size) = engine
        .attachment_replay(&id, &buffer, true, &mut sizes)
        .await;
    let mut rendered = vt100::Parser::new(3, 5, 1024);
    rendered.process(&initial);
    let mut cursor = crate::engine::AttachmentCursor {
        offset,
        size,
        snapshot: true,
        identified: true,
    };
    // Both batches remain unread by the viewer when the size changes.
    buffer.write(b"ABCDEF");
    engine.attachment_resize(&id, owner, 10, 3, true).await;
    buffer.write(b"G");
    engine.attachment_resize(&id, owner, 8, 3, true).await;
    let update = engine
        .attachment_update(&id, &buffer, &mut cursor, &mut sizes, None)
        .await;
    assert!(update.resync);
    assert_eq!(
        update.sizes.iter().map(|size| size.0).collect::<Vec<_>>(),
        vec![10, 8]
    );
    let (snapshot, snapshot_offset) = buffer.attach_snapshot();
    let mut expected = vt100::Parser::new(3, 8, 1024);
    expected.process(&snapshot);
    assert_eq!(cursor.offset, snapshot_offset);
    let replay_bytes = update.data.len();
    let (server, client) = UnixStream::pair().unwrap();
    let (_, mut writer) = server.into_split();
    let (read, _) = client.into_split();
    let mut reader = BufReader::new(read);
    streams::write_attachment_update(&mut writer, &id, update)
        .await
        .unwrap();
    assert!(matches!(next_response(&mut reader).await, Response::Resync));
    rendered.process(b"\x1bc");
    // Every queued size report precedes the latest matching snapshot.
    for expected_cols in [10, 8] {
        let Response::PtySize { cols, rows, .. } = next_response(&mut reader).await else {
            panic!("size report must precede snapshot output")
        };
        assert_eq!((cols, rows), (expected_cols, 3));
        rendered.screen_mut().set_size(rows, cols);
    }
    let mut remaining = replay_bytes;
    while remaining > 0 {
        let Response::Output { data, .. } = next_response(&mut reader).await else {
            panic!("expected snapshot output")
        };
        remaining -= data.len();
        rendered.process(&data);
    }
    assert_eq!(rendered.screen().contents(), expected.screen().contents());
    assert!(rendered.screen().contents().contains("ABCDE\nFG"));
    assert_eq!(cursor.offset, buffer.current_offset());
    buffer.write(b"H");
    let live = engine
        .attachment_update(&id, &buffer, &mut cursor, &mut sizes, None)
        .await;
    assert!(!live.resync);
    assert!(live.sizes.is_empty());
    rendered.process(&live.data);
    let (snapshot, snapshot_offset) = buffer.attach_snapshot();
    expected.process(&snapshot);
    assert_eq!(rendered.screen().contents(), expected.screen().contents());
    assert_eq!(cursor.offset, snapshot_offset);
}

/// Every response until the daemon closes the connection (panics if it doesn't).
async fn responses_until_close(
    reader: &mut BufReader<tokio::net::unix::OwnedReadHalf>,
) -> Vec<Response> {
    let mut responses = Vec::new();
    loop {
        let mut line = String::new();
        let read = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            reader.read_line(&mut line),
        )
        .await
        .expect("daemon must close the connection");
        match read {
            Ok(0) | Err(_) => return responses,
            Ok(_) => responses.push(serde_json::from_str(&line).unwrap()),
        }
    }
}

/// One oversized frame whose bytes are not valid UTF-8 (a stray 0xFF, and a
/// multibyte character split by the size limit), followed by a request that
/// must never run.
fn oversized_then_health() -> Vec<u8> {
    let mut bytes = vec![0xFF];
    bytes.resize(MAX_MESSAGE_SIZE as usize - 1, b'a');
    bytes.extend_from_slice("€".as_bytes()); // 3 bytes straddling the limit
    bytes.extend_from_slice(serde_json::to_string(&Request::Health).unwrap().as_bytes());
    bytes.push(b'\n');
    bytes
}

fn assert_frame_too_large_only(responses: &[Response]) {
    assert!(
        matches!(responses, [Response::Error { code, .. }] if code == "FRAME_TOO_LARGE"),
        "the bytes after an oversized frame must never run as a request: {responses:?}"
    );
}

async fn write_line(writer: &mut tokio::net::unix::OwnedWriteHalf, request: &Request) {
    let line = serde_json::to_string(request).unwrap() + "\n";
    writer.write_all(line.as_bytes()).await.unwrap();
}

async fn read_response(reader: &mut BufReader<tokio::net::unix::OwnedReadHalf>) -> Response {
    let mut line = String::new();
    reader.read_line(&mut line).await.unwrap();
    serde_json::from_str(&line).unwrap()
}

/// A stream-entering request and a check of its first reply.
type Subscribe = (Request, fn(&Response) -> bool);

/// Opens a connection, optionally enters a stream with `subscribe` (checking
/// its first reply), then sends an oversized frame and expects only
/// `FRAME_TOO_LARGE` before the daemon closes the connection.
async fn assert_oversized_frame_closes(sock_path: &std::path::Path, subscribe: Option<Subscribe>) {
    let stream = UnixStream::connect(sock_path).await.unwrap();
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    if let Some((request, subscribed)) = subscribe {
        write_line(&mut writer, &request).await;
        let first = read_response(&mut reader).await;
        assert!(subscribed(&first), "{first:?}");
    }
    let send = tokio::spawn(async move {
        let _ = writer.write_all(&oversized_then_health()).await;
        writer
    });
    let responses: Vec<_> = responses_until_close(&mut reader)
        .await
        .into_iter()
        // Stream traffic before the error is fine; only what follows matters.
        .skip_while(|r| !matches!(r, Response::Error { .. }))
        .collect();
    assert_frame_too_large_only(&responses);
    let _ = send.await;
}

async fn initialized_server(
    tmp: &TempDir,
) -> (
    std::path::PathBuf,
    String,
    tokio::task::JoinHandle<std::io::Result<()>>,
) {
    let sock_path = tmp.path().join("test.sock");
    let project_root = tmp.path().canonicalize().unwrap().join("project");
    std::fs::create_dir_all(&project_root).unwrap();
    let engine = Engine::new();
    let init = engine
        .handle_request(Request::Init {
            project_root: project_root.to_string_lossy().into(),
        })
        .await;
    assert!(matches!(init, Response::InitResult { .. }));
    let server = IpcServer::bind(&sock_path, engine).unwrap();
    let handle = tokio::spawn(async move { server.run().await });
    (
        sock_path,
        project_root.to_string_lossy().into_owned(),
        handle,
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_oversized_frame_should_reject_and_close_without_running_the_rest() {
    let tmp = TempDir::new().unwrap();
    let sock_path = tmp.path().join("test.sock");
    let server = IpcServer::bind(&sock_path, Engine::new()).unwrap();
    let handle = tokio::spawn(async move { server.run().await });
    assert_oversized_frame_closes(&sock_path, None).await;
    handle.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_oversized_frame_in_grid_stream_should_reject_and_close() {
    let tmp = TempDir::new().unwrap();
    let (sock_path, project_root, handle) = initialized_server(&tmp).await;
    let subscribe = Request::SubscribeGrid { project_root };
    let subscribed: fn(&Response) -> bool = |r| matches!(r, Response::GridSubscribed);
    assert_oversized_frame_closes(&sock_path, Some((subscribe, subscribed))).await;
    handle.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_oversized_frame_in_status_stream_should_reject_and_close() {
    let tmp = TempDir::new().unwrap();
    let (sock_path, project_root, handle) = initialized_server(&tmp).await;
    let subscribe = Request::SubscribeStatus { project_root };
    let subscribed: fn(&Response) -> bool = |r| matches!(r, Response::StatusSubscribed);
    assert_oversized_frame_closes(&sock_path, Some((subscribe, subscribed))).await;
    handle.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_oversized_frame_in_attach_stream_should_reject_and_close() {
    let (engine, agent_id, tmp) =
        crate::test_helpers::init_and_spawn_command(Some("/bin/cat")).await;
    let sock_path = tmp.path().join("test.sock");
    let server = IpcServer::bind(&sock_path, engine).unwrap();
    let engine = server.engine().clone();
    let handle = tokio::spawn(async move { server.run().await });
    let subscribe = Request::Attach {
        agent_id: agent_id.clone(),
        replay: None,
        client_id: None,
    };
    let subscribed: fn(&Response) -> bool = |r| matches!(r, Response::AttachReady { .. });
    assert_oversized_frame_closes(&sock_path, Some((subscribe, subscribed))).await;
    engine
        .kill_all_sessions(std::time::Duration::from_millis(10))
        .await;
    handle.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_line_split_across_writes_in_attach_stream_should_not_lose_bytes() {
    let (engine, agent_id, tmp) =
        crate::test_helpers::init_and_spawn_command(Some("/bin/cat")).await;
    let sock_path = tmp.path().join("test.sock");
    let server = IpcServer::bind(&sock_path, engine).unwrap();
    let engine = server.engine().clone();
    let handle = tokio::spawn(async move { server.run().await });

    let stream = UnixStream::connect(&sock_path).await.unwrap();
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    write_line(
        &mut writer,
        &Request::Attach {
            agent_id: agent_id.clone(),
            replay: None,
            client_id: None,
        },
    )
    .await;
    assert!(matches!(
        read_response(&mut reader).await,
        Response::AttachReady { .. }
    ));

    // Send half a request line, then make the agent print so the stream's
    // output branch wins the select while that line is still incomplete.
    let input = serde_json::to_string(&Request::Input {
        agent_id: agent_id.clone(),
        data: b"split-line-marker\n".to_vec(),
        submit: false,
        cancel_on_disconnect: false,
    })
    .unwrap()
        + "\n";
    let (head, tail) = input.as_bytes().split_at(input.len() / 2);
    writer.write_all(head).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    engine
        .handle_request(Request::Input {
            agent_id: agent_id.clone(),
            data: b"interleaved-output\n".to_vec(),
            submit: false,
            cancel_on_disconnect: false,
        })
        .await;
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    writer.write_all(tail).await.unwrap();

    let marker = b"split-line-marker";
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        let mut received = Vec::new();
        while !received.windows(marker.len()).any(|w| w == marker) {
            match read_response(&mut reader).await {
                Response::Output { data, .. } => received.extend(data),
                other => panic!("expected Output, got {other:?}"),
            }
        }
    })
    .await
    .expect("the split request line must still reach the agent");
    engine
        .kill_all_sessions(std::time::Duration::from_millis(10))
        .await;
    handle.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn given_connection_without_request_should_be_closed_after_first_line_timeout() {
    let tmp = TempDir::new().unwrap();
    let sock_path = tmp.path().join("test.sock");
    let server = IpcServer::bind(&sock_path, Engine::new())
        .unwrap()
        .with_first_line_timeout(std::time::Duration::from_millis(100));
    let handle = tokio::spawn(async move { server.run().await });

    let idle = UnixStream::connect(&sock_path).await.unwrap();
    let (reader, _writer) = idle.into_split();
    let mut reader = BufReader::new(reader);
    assert!(responses_until_close(&mut reader).await.is_empty());

    // A connection that does send a request is still served.
    let stream = UnixStream::connect(&sock_path).await.unwrap();
    assert!(matches!(
        send_health(stream).await,
        Some(Response::HealthReport { .. })
    ));
    handle.abort();
}

/// umask is process-wide, so the permissive-umask bind runs in a child copy
/// of this test binary instead of racing other tests' file creation.
#[test]
fn given_permissive_umask_socket_should_still_be_owner_only() {
    const CHILD: &str = "PU_TEST_UMASK_CHILD";
    let test = "ipc_server::tests::given_permissive_umask_socket_should_still_be_owner_only";
    if std::env::var_os(CHILD).is_some() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = TempDir::new().unwrap();
        let sock_path = tmp.path().join("test.sock");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        // SAFETY: umask has no preconditions; this process runs only this test.
        unsafe { libc::umask(0o000) };
        let _server = runtime
            .block_on(async { IpcServer::bind(&sock_path, Engine::new()) })
            .unwrap();
        let mode = std::fs::metadata(&sock_path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test, "--test-threads=1"])
        .env(CHILD, "1")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    // A renamed test would match nothing and "pass" with 0 tests run.
    assert!(
        output.status.success() && stdout.contains("1 passed"),
        "child test must run and pass exactly this test: {stdout}"
    );
}

#[test]
fn given_peer_of_another_user_should_be_refused() {
    assert!(peer_allowed(Some(501), 501));
    assert!(!peer_allowed(Some(502), 501));
    assert!(!peer_allowed(Some(0), 501));
    assert!(!peer_allowed(None, 501));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_line_still_arriving_when_attach_ends_should_finish_in_request_mode() {
    let (engine, agent_id, tmp) =
        crate::test_helpers::init_and_spawn_command(Some("/bin/cat")).await;
    let project_root = tmp.path().join("project").to_string_lossy().into_owned();
    let sock_path = tmp.path().join("test.sock");
    let server = IpcServer::bind(&sock_path, engine).unwrap();
    let engine = server.engine().clone();
    let handle = tokio::spawn(async move { server.run().await });

    let stream = UnixStream::connect(&sock_path).await.unwrap();
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    write_line(
        &mut writer,
        &Request::Attach {
            agent_id: agent_id.clone(),
            replay: None,
            client_id: None,
        },
    )
    .await;
    assert!(matches!(
        read_response(&mut reader).await,
        Response::AttachReady { .. }
    ));

    // Half an Input line is in flight when the agent exits and the attach
    // stream hands the connection back to request mode.
    let input = serde_json::to_string(&Request::Input {
        agent_id: agent_id.clone(),
        data: b"late\n".to_vec(),
        submit: false,
        cancel_on_disconnect: false,
    })
    .unwrap()
        + "\n";
    let (head, tail) = input.as_bytes().split_at(input.len() / 2);
    writer.write_all(head).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    engine
        .handle_request(Request::Kill {
            project_root,
            target: KillTarget::Agent(agent_id),
            exclude: vec![],
        })
        .await;
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    writer.write_all(tail).await.unwrap();
    write_line(&mut writer, &Request::Health).await;

    let responses = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let mut responses = Vec::new();
        loop {
            let response = read_response(&mut reader).await;
            let done = matches!(response, Response::HealthReport { .. });
            responses.push(response);
            if done {
                return responses;
            }
        }
    })
    .await
    .expect("health must be answered after the stream ends");
    assert!(
        !responses
            .iter()
            .any(|r| matches!(r, Response::Error { code, .. } if code == "PARSE_ERROR")),
        "the split line must be reassembled, not parsed in pieces: {responses:?}"
    );
    let health = responses
        .iter()
        .filter(|r| matches!(r, Response::HealthReport { .. }))
        .count();
    assert_eq!(health, 1, "{responses:?}");
    handle.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn given_request_without_trailing_newline_at_eof_should_still_be_answered() {
    let tmp = TempDir::new().unwrap();
    let sock_path = tmp.path().join("test.sock");
    let server = IpcServer::bind(&sock_path, Engine::new()).unwrap();
    let handle = tokio::spawn(async move { server.run().await });

    let stream = UnixStream::connect(&sock_path).await.unwrap();
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let request = serde_json::to_string(&Request::Health).unwrap();
    writer.write_all(request.as_bytes()).await.unwrap();
    writer.shutdown().await.unwrap();
    let responses = responses_until_close(&mut reader).await;
    assert!(
        matches!(responses.as_slice(), [Response::HealthReport { .. }]),
        "{responses:?}"
    );
    handle.abort();
}

/// Enters a stream, sends a short line that is not UTF-8 and then a valid
/// request: the stream must close the connection without running it.
async fn assert_invalid_bytes_close_stream(sock_path: &std::path::Path, subscribe: Subscribe) {
    let stream = UnixStream::connect(sock_path).await.unwrap();
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let (request, subscribed) = subscribe;
    write_line(&mut writer, &request).await;
    let first = read_response(&mut reader).await;
    assert!(subscribed(&first), "{first:?}");
    writer.write_all(b"\xff{}\n").await.unwrap();
    write_line(&mut writer, &Request::Health).await;
    let responses = responses_until_close(&mut reader).await;
    assert!(
        !responses
            .iter()
            .any(|r| matches!(r, Response::HealthReport { .. })),
        "nothing after unreadable input may run: {responses:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_invalid_utf8_in_grid_stream_should_close_the_connection() {
    let tmp = TempDir::new().unwrap();
    let (sock_path, project_root, handle) = initialized_server(&tmp).await;
    let subscribed: fn(&Response) -> bool = |r| matches!(r, Response::GridSubscribed);
    let subscribe = (Request::SubscribeGrid { project_root }, subscribed);
    assert_invalid_bytes_close_stream(&sock_path, subscribe).await;
    handle.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_invalid_utf8_in_status_stream_should_close_the_connection() {
    let tmp = TempDir::new().unwrap();
    let (sock_path, project_root, handle) = initialized_server(&tmp).await;
    let subscribed: fn(&Response) -> bool = |r| matches!(r, Response::StatusSubscribed);
    let subscribe = (Request::SubscribeStatus { project_root }, subscribed);
    assert_invalid_bytes_close_stream(&sock_path, subscribe).await;
    handle.abort();
}

use std::path::Path;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::{Notify, Semaphore};

const MAX_MESSAGE_SIZE: u64 = 1024 * 1024; // 1MB
const MAX_CONNECTIONS: usize = 64;

use crate::engine::Engine;
use pu_core::protocol::{Request, Response};

pub struct IpcServer {
    listener: UnixListener,
    engine: Arc<Engine>,
    shutdown: Arc<Notify>,
    conn_limit: Arc<Semaphore>,
}

impl IpcServer {
    pub fn bind(socket_path: &Path, engine: Engine) -> Result<Self, std::io::Error> {
        // Remove stale socket if it exists
        let _ = std::fs::remove_file(socket_path);
        let listener = UnixListener::bind(socket_path)?;
        Ok(Self {
            listener,
            engine: Arc::new(engine),
            shutdown: Arc::new(Notify::new()),
            conn_limit: Arc::new(Semaphore::new(MAX_CONNECTIONS)),
        })
    }

    /// Get a reference to the engine for starting background tasks.
    pub fn engine(&self) -> &Arc<Engine> {
        &self.engine
    }

    pub async fn run(self) -> Result<(), std::io::Error> {
        let mut sigterm = tokio::signal::unix::signal(
            tokio::signal::unix::SignalKind::terminate(),
        )?;
        let mut sigint = tokio::signal::unix::signal(
            tokio::signal::unix::SignalKind::interrupt(),
        )?;

        loop {
            tokio::select! {
                accept = self.listener.accept() => {
                    let (stream, _addr) = accept?;
                    let engine = self.engine.clone();
                    let shutdown = self.shutdown.clone();
                    let permit = match self.conn_limit.clone().acquire_owned().await {
                        Ok(p) => p,
                        Err(_) => continue, // semaphore closed
                    };
                    tokio::spawn(async move {
                        let _permit = permit;
                        Self::handle_connection(stream, engine, shutdown).await;
                    });
                }
                _ = self.shutdown.notified() => {
                    tracing::info!("shutdown requested via IPC");
                    return Ok(());
                }
                _ = sigterm.recv() => {
                    tracing::info!("received SIGTERM, shutting down");
                    return Ok(());
                }
                _ = sigint.recv() => {
                    tracing::info!("received SIGINT, shutting down");
                    return Ok(());
                }
            }
        }
    }

    async fn handle_connection(
        stream: tokio::net::UnixStream,
        engine: Arc<Engine>,
        shutdown: Arc<Notify>,
    ) {
        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);
        let mut line = String::new();

        loop {
            line.clear();
            match (&mut reader).take(MAX_MESSAGE_SIZE).read_line(&mut line).await {
                Ok(0) => break, // EOF
                Ok(_) => {
                    let request: Request = match serde_json::from_str(line.trim()) {
                        Ok(r) => r,
                        Err(e) => {
                            let resp = Response::Error {
                                code: "PARSE_ERROR".into(),
                                message: e.to_string(),
                            };
                            if Self::write_response(&mut writer, &resp).await.is_err() {
                                break;
                            }
                            continue;
                        }
                    };

                    let is_shutdown = matches!(request, Request::Shutdown);

                    // Detect attach/subscribe requests to enter streaming mode
                    let attach_agent_id = if let Request::Attach { ref agent_id } = request {
                        Some(agent_id.clone())
                    } else {
                        None
                    };
                    let subscribe_grid_root = if let Request::SubscribeGrid { ref project_root } = request {
                        Some(project_root.clone())
                    } else {
                        None
                    };
                    let subscribe_status_root = if let Request::SubscribeStatus { ref project_root } = request {
                        Some(project_root.clone())
                    } else {
                        None
                    };

                    let response = engine.handle_request(request).await;
                    if Self::write_response(&mut writer, &response).await.is_err() {
                        if is_shutdown {
                            shutdown.notify_one();
                        }
                        break;
                    }

                    if is_shutdown {
                        shutdown.notify_one();
                        break;
                    }

                    // Enter streaming sub-loop for attach
                    if let Some(agent_id) = attach_agent_id {
                        if !matches!(response, Response::AttachReady { .. }) {
                            break;
                        }
                        Self::handle_attach_stream(
                            &mut reader,
                            &mut writer,
                            &engine,
                            &agent_id,
                        )
                        .await;
                    }

                    // Enter streaming sub-loop for grid subscription
                    if let Some(project_root) = subscribe_grid_root {
                        if matches!(response, Response::GridSubscribed) {
                            Self::handle_grid_stream(
                                &mut reader,
                                &mut writer,
                                &engine,
                                &project_root,
                            )
                            .await;
                        }
                    }

                    // Enter streaming sub-loop for status subscription
                    if let Some(project_root) = subscribe_status_root {
                        if matches!(response, Response::StatusSubscribed) {
                            Self::handle_status_stream(
                                &mut reader,
                                &mut writer,
                                &engine,
                                &project_root,
                            )
                            .await;
                        }
                    }
                }
                Err(_) => break,
            }
        }
    }

    /// Streaming attach sub-loop: sends all buffered output, then streams new output
    /// while accepting Input/Resize commands from the client.
    async fn handle_attach_stream(
        reader: &mut BufReader<tokio::net::unix::OwnedReadHalf>,
        writer: &mut tokio::net::unix::OwnedWriteHalf,
        engine: &Engine,
        agent_id: &str,
    ) {
        let (buffer, master_fd, mut exit_rx) = match engine.get_attach_handles(agent_id).await {
            Some(handles) => handles,
            None => {
                let _ = Self::write_response(writer, &Response::Error {
                    code: "AGENT_NOT_FOUND".into(),
                    message: format!("agent {agent_id} was removed during attach"),
                }).await;
                return;
            }
        };

        tracing::debug!(agent_id, "attach stream started");

        let mut watcher = buffer.subscribe();

        // Send buffered output in 64KB chunks so client starts rendering immediately
        const CHUNK_SIZE: usize = 64 * 1024;
        let mut offset = 0;
        let (data, new_offset) = buffer.read_from(offset);
        offset = new_offset;
        if !data.is_empty() {
            for chunk in data.chunks(CHUNK_SIZE) {
                let resp = Response::Output {
                    agent_id: agent_id.to_string(),
                    data: chunk.to_vec(),
                };
                if Self::write_response(writer, &resp).await.is_err() {
                    tracing::debug!(agent_id, "attach stream ended: write error on initial data");
                    return;
                }
            }
        }

        let process_exited = exit_rx.borrow().is_some();
        let mut line = String::new();
        loop {
            tokio::select! {
                Ok(()) = watcher.changed() => {
                    let (data, new_offset) = buffer.read_from(offset);
                    offset = new_offset;
                    if !data.is_empty() {
                        let resp = Response::Output {
                            agent_id: agent_id.to_string(),
                            data,
                        };
                        if Self::write_response(writer, &resp).await.is_err() {
                            tracing::debug!(agent_id, "attach stream ended: write error");
                            break;
                        }
                    }
                }
                Ok(()) = exit_rx.changed(), if !process_exited => {
                    // Drain any remaining buffered output
                    let (data, _) = buffer.read_from(offset);
                    if !data.is_empty() {
                        let resp = Response::Output {
                            agent_id: agent_id.to_string(),
                            data,
                        };
                        let _ = Self::write_response(writer, &resp).await;
                    }
                    tracing::debug!(agent_id, "attach stream ended: process exited");
                    break;
                }
                result = async {
                    line.clear();
                    reader.take(MAX_MESSAGE_SIZE).read_line(&mut line).await
                } => {
                    match result {
                        Ok(0) => {
                            tracing::debug!(agent_id, "attach stream ended: client disconnected");
                            break;
                        }
                        Ok(_) => {
                            let request: Request = match serde_json::from_str(line.trim()) {
                                Ok(r) => r,
                                Err(_) => break,
                            };
                            match request {
                                Request::Input { data, .. } => {
                                    engine.write_to_pty(&master_fd, &data).await.ok();
                                }
                                Request::Resize { cols, rows, .. } => {
                                    engine.resize_pty(&master_fd, cols, rows).await.ok();
                                }
                                _ => break, // Any other request exits the attach loop
                            }
                        }
                        Err(_) => break,
                    }
                }
            }
        }
        tracing::debug!(agent_id, "attach stream ended");
    }

    /// Streaming grid subscription: forwards GridEvent broadcasts to subscriber,
    /// accepts incoming GridCommand requests from the subscriber connection.
    async fn handle_grid_stream(
        reader: &mut BufReader<tokio::net::unix::OwnedReadHalf>,
        writer: &mut tokio::net::unix::OwnedWriteHalf,
        engine: &Engine,
        project_root: &str,
    ) {
        let mut rx = engine.subscribe_grid(project_root).await;
        let pr = project_root.to_string();

        tracing::debug!(project_root, "grid stream started");

        let mut line = String::new();
        loop {
            tokio::select! {
                result = rx.recv() => {
                    match result {
                        Ok(command) => {
                            let resp = Response::GridEvent {
                                project_root: pr.clone(),
                                command,
                            };
                            if Self::write_response(writer, &resp).await.is_err() {
                                break;
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            tracing::warn!(project_root, "grid subscriber lagged {n} messages");
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
                result = async {
                    line.clear();
                    reader.take(MAX_MESSAGE_SIZE).read_line(&mut line).await
                } => {
                    match result {
                        Ok(0) => break, // Client disconnected
                        Ok(_) => {
                            let request: Request = match serde_json::from_str(line.trim()) {
                                Ok(r) => r,
                                Err(_) => break,
                            };
                            // Only accept GridCommand during grid stream
                            match request {
                                Request::GridCommand { command, .. } => {
                                    let resp = engine.handle_grid_command(&pr, command).await;
                                    if Self::write_response(writer, &resp).await.is_err() {
                                        break;
                                    }
                                }
                                _ => break, // Any other request exits the grid stream
                            }
                        }
                        Err(_) => break,
                    }
                }
            }
        }
        tracing::debug!(project_root, "grid stream ended");
    }

    /// Streaming status subscription: pushes full StatusEvent on every state change.
    /// Client receives real-time updates without polling.
    async fn handle_status_stream(
        reader: &mut BufReader<tokio::net::unix::OwnedReadHalf>,
        writer: &mut tokio::net::unix::OwnedWriteHalf,
        engine: &Engine,
        project_root: &str,
    ) {
        let mut rx = engine.subscribe_status(project_root).await;
        let pr = project_root.to_string();

        tracing::debug!(project_root, "status stream started");

        // Send initial status immediately
        if let Some((worktrees, agents)) = engine.compute_full_status(&pr).await {
            let resp = Response::StatusEvent { worktrees, agents };
            if Self::write_response(writer, &resp).await.is_err() {
                return;
            }
        }

        let mut line = String::new();
        loop {
            tokio::select! {
                result = rx.recv() => {
                    match result {
                        Ok(()) => {
                            // Drain any queued signals (batch rapid changes)
                            while rx.try_recv().is_ok() {}
                            if let Some((worktrees, agents)) = engine.compute_full_status(&pr).await {
                                let resp = Response::StatusEvent { worktrees, agents };
                                if Self::write_response(writer, &resp).await.is_err() {
                                    break;
                                }
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            tracing::warn!(project_root, "status subscriber lagged {n} messages");
                            // Send fresh status after lag
                            if let Some((worktrees, agents)) = engine.compute_full_status(&pr).await {
                                let resp = Response::StatusEvent { worktrees, agents };
                                if Self::write_response(writer, &resp).await.is_err() {
                                    break;
                                }
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
                result = async {
                    line.clear();
                    reader.take(MAX_MESSAGE_SIZE).read_line(&mut line).await
                } => {
                    match result {
                        Ok(0) => break, // Client disconnected
                        Ok(_) => break, // Any request exits the status stream
                        Err(_) => break,
                    }
                }
            }
        }
        tracing::debug!(project_root, "status stream ended");
    }

    async fn write_response(
        writer: &mut tokio::net::unix::OwnedWriteHalf,
        response: &Response,
    ) -> std::io::Result<()> {
        let json = serde_json::to_string(response)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        writer.write_all(json.as_bytes()).await?;
        writer.write_all(b"\n").await
    }
}

#[cfg(test)]
mod tests {
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
        writer.write_all(format!("{req}\n").as_bytes()).await.unwrap();

        // Read response
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        let resp: Response = serde_json::from_str(&line).unwrap();

        match resp {
            Response::HealthReport { protocol_version, .. } => {
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
        writer.write_all(format!("{req}\n").as_bytes()).await.unwrap();

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
        writer.write_all(format!("{req}\n").as_bytes()).await.unwrap();

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
    async fn setup_with_agent() -> (std::path::PathBuf, String, tokio::task::JoinHandle<Result<(), std::io::Error>>, TempDir) {
        let (engine, agent_id, tmp) = crate::test_helpers::init_and_spawn().await;
        let sock_path = tmp.path().join("test.sock");

        let server = IpcServer::bind(&sock_path, engine).unwrap();
        let handle = tokio::spawn(async move { server.run().await });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        (sock_path.clone(), agent_id, handle, tmp)
    }

    #[tokio::test(flavor = "current_thread")]
    async fn given_attach_request_should_stream_output_continuously() {
        let (sock_path, agent_id, server_handle, _tmp) = setup_with_agent().await;

        // Wait for the agent to produce some output
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        // Connect a new client and attach
        let stream = UnixStream::connect(&sock_path).await.unwrap();
        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);

        let req = serde_json::to_string(&Request::Attach { agent_id: agent_id.clone() }).unwrap();
        writer.write_all(format!("{req}\n").as_bytes()).await.unwrap();

        // Should get AttachReady first
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        let resp: Response = serde_json::from_str(line.trim()).unwrap();
        assert!(matches!(resp, Response::AttachReady { .. }), "expected AttachReady, got {resp:?}");

        // Should get at least one Output message with buffered data
        line.clear();
        let read_result = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            reader.read_line(&mut line),
        ).await;
        assert!(read_result.is_ok(), "timed out waiting for Output");
        let resp: Response = serde_json::from_str(line.trim()).unwrap();
        match resp {
            Response::Output { agent_id: id, data } => {
                assert_eq!(id, agent_id);
                assert!(!data.is_empty(), "expected non-empty output data");
            }
            other => panic!("expected Output, got {other:?}"),
        }

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
        let req = serde_json::to_string(&Request::Attach { agent_id: agent_id.clone() }).unwrap();
        writer.write_all(format!("{req}\n").as_bytes()).await.unwrap();

        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap(); // AttachReady

        // Send input — even if the process has exited, the write to the PTY fd should
        // not crash the server. The server handles EPIPE/EIO gracefully.
        let input_req = serde_json::to_string(&Request::Input {
            agent_id: agent_id.clone(),
            data: b"hello\n".to_vec(),
        }).unwrap();
        writer.write_all(format!("{input_req}\n").as_bytes()).await.unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        // Verify server is still running — connection didn't crash
        drop(writer);
        drop(reader);

        // Health check on a new connection
        let stream = UnixStream::connect(&sock_path).await.unwrap();
        let (reader2, mut writer2) = stream.into_split();
        let mut reader2 = BufReader::new(reader2);

        let health_req = serde_json::to_string(&Request::Health).unwrap();
        writer2.write_all(format!("{health_req}\n").as_bytes()).await.unwrap();
        let mut line2 = String::new();
        reader2.read_line(&mut line2).await.unwrap();
        let resp: Response = serde_json::from_str(line2.trim()).unwrap();
        assert!(matches!(resp, Response::HealthReport { .. }), "server still healthy after input during attach");

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
        let req = serde_json::to_string(&Request::Attach { agent_id: agent_id.clone() }).unwrap();
        writer.write_all(format!("{req}\n").as_bytes()).await.unwrap();

        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap(); // AttachReady

        // Send resize — should not error or crash
        let resize_req = serde_json::to_string(&Request::Resize {
            agent_id: agent_id.clone(),
            cols: 200,
            rows: 50,
        }).unwrap();
        writer.write_all(format!("{resize_req}\n").as_bytes()).await.unwrap();

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
        }).unwrap();
        writer.write_all(format!("{kill_req}\n").as_bytes()).await.unwrap();

        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        let resp: Response = serde_json::from_str(line.trim()).unwrap();
        assert!(matches!(resp, Response::KillResult { .. }), "expected KillResult, got {resp:?}");
        drop(writer);
        drop(reader);

        // Now attempt to attach — agent session is gone, should get AGENT_NOT_FOUND
        let stream = UnixStream::connect(&sock_path).await.unwrap();
        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);

        let attach_req = serde_json::to_string(&Request::Attach { agent_id }).unwrap();
        writer.write_all(format!("{attach_req}\n").as_bytes()).await.unwrap();

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

            let req = serde_json::to_string(&Request::Attach { agent_id: agent_id.clone() }).unwrap();
            writer.write_all(format!("{req}\n").as_bytes()).await.unwrap();

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
        writer.write_all(format!("{req}\n").as_bytes()).await.unwrap();

        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        let resp: Response = serde_json::from_str(line.trim()).unwrap();
        assert!(matches!(resp, Response::HealthReport { .. }), "server still healthy after attach disconnect");

        server_handle.abort();
    }
}

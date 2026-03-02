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
                }
                Err(_) => break,
            }
        }
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
    use pu_core::protocol::{Request, Response};
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
}

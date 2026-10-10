use std::path::Path;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::{Notify, Semaphore};

const MAX_MESSAGE_SIZE: u64 = 1024 * 1024; // 1MB
/// The app holds a status and a grid stream per open project plus one attach
/// stream per terminal pane, all for as long as they are open, so this must sit
/// well above what a busy session uses.
const MAX_CONNECTIONS: usize = 1024;
const ATTACH_OUTPUT_CHUNK_SIZE: usize = 64 * 1024;

use crate::engine::Engine;
use pu_core::protocol::{Request, Response};

type IpcReader = BufReader<tokio::net::unix::OwnedReadHalf>;
type IpcWriter = tokio::net::unix::OwnedWriteHalf;

mod streams;

#[cfg(test)]
mod tests;

enum StreamMode {
    None,
    Attach(String),
    Grid(String),
    Status(String),
}

impl StreamMode {
    fn from_request(request: &Request) -> Self {
        match request {
            Request::Attach { agent_id } => Self::Attach(agent_id.clone()),
            Request::SubscribeGrid { project_root } => Self::Grid(project_root.clone()),
            Request::SubscribeStatus { project_root } => Self::Status(project_root.clone()),
            _ => Self::None,
        }
    }
}

fn parse_stream_request(result: std::io::Result<usize>, line: &str) -> Option<Request> {
    match result {
        Ok(0) | Err(_) => None,
        Ok(_) => serde_json::from_str(line.trim()).ok(),
    }
}

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

    /// Override the concurrent connection cap (defaults to `MAX_CONNECTIONS`).
    #[cfg(test)]
    pub(crate) fn with_connection_limit(mut self, limit: usize) -> Self {
        self.conn_limit = Arc::new(Semaphore::new(limit));
        self
    }

    /// Get a reference to the engine for starting background tasks.
    pub fn engine(&self) -> &Arc<Engine> {
        &self.engine
    }

    pub async fn run(self) -> Result<(), std::io::Error> {
        let mut sigterm =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        let mut sigint = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;

        loop {
            tokio::select! {
                accept = self.listener.accept() => {
                    // A failed accept (e.g. EMFILE) is transient: never let it end
                    // the server and strand every running agent.
                    let stream = match accept {
                        Ok((stream, _addr)) => stream,
                        Err(e) => {
                            tracing::warn!("accept failed: {e}");
                            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                            continue;
                        }
                    };
                    // Never wait for a permit: waiting here would stall accepting,
                    // shutdown and signals, and parking the stream in a task would
                    // let stalled clients pile up descriptors past the cap.
                    let Ok(permit) = self.conn_limit.clone().try_acquire_owned() else {
                        tracing::warn!("connection limit reached; rejecting connection");
                        reject_busy(&stream);
                        continue; // dropping the stream closes it
                    };
                    let engine = self.engine.clone();
                    let shutdown = self.shutdown.clone();
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
        let (reader, writer) = stream.into_split();
        let mut reader = BufReader::new(reader);
        let mut writer = writer;
        let mut line = String::new();

        loop {
            line.clear();
            match (&mut reader)
                .take(MAX_MESSAGE_SIZE)
                .read_line(&mut line)
                .await
            {
                Ok(0) => break, // EOF
                Ok(_) => {
                    if !Self::dispatch_request(&line, &mut reader, &mut writer, &engine, &shutdown)
                        .await
                    {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    }

    /// Handles a single parsed request line. Returns `true` to continue the
    /// connection loop, `false` to break.
    async fn dispatch_request(
        line: &str,
        reader: &mut IpcReader,
        writer: &mut IpcWriter,
        engine: &Engine,
        shutdown: &Notify,
    ) -> bool {
        let mut request: Request = match serde_json::from_str(line.trim()) {
            Ok(r) => r,
            Err(e) => {
                let resp = Response::Error {
                    code: "PARSE_ERROR".into(),
                    message: e.to_string(),
                };
                return write_response(writer, &resp).await.is_ok();
            }
        };

        if let Err(response) = engine.prepare_request(&mut request).await {
            return write_response(writer, &response).await.is_ok();
        }
        let is_shutdown = matches!(request, Request::Shutdown);
        let stream_mode = StreamMode::from_request(&request);

        let response = engine.handle_prepared_request(request).await;
        if write_response(writer, &response).await.is_err() {
            if is_shutdown {
                shutdown.notify_one();
            }
            return false;
        }

        if is_shutdown {
            shutdown.notify_one();
            return false;
        }

        match stream_mode {
            StreamMode::Attach(agent_id) => {
                if !matches!(response, Response::AttachReady { .. }) {
                    return false;
                }
                streams::handle_attach_stream(reader, writer, engine, &agent_id).await;
            }
            StreamMode::Grid(project_root) => {
                if matches!(response, Response::GridSubscribed) {
                    streams::handle_grid_stream(reader, writer, engine, &project_root).await;
                }
            }
            StreamMode::Status(project_root) => {
                if matches!(response, Response::StatusSubscribed) {
                    streams::handle_status_stream(reader, writer, engine, &project_root).await;
                }
            }
            StreamMode::None => {}
        }

        true
    }
}

/// Best-effort, non-blocking BUSY reply to a connection over the limit; a
/// partial or failed write is ignored since the stream is closed right after.
fn reject_busy(stream: &tokio::net::UnixStream) {
    let resp = Response::Error {
        code: "BUSY".into(),
        message: "daemon connection limit reached".into(),
    };
    if let Ok(mut json) = serde_json::to_vec(&resp) {
        json.push(b'\n');
        let _ = stream.try_write(&json);
    }
}

async fn write_response(writer: &mut IpcWriter, response: &Response) -> std::io::Result<()> {
    let mut json = serde_json::to_vec(response)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    json.push(b'\n');
    writer.write_all(&json).await
}

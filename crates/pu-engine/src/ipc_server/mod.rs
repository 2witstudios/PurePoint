use std::path::Path;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{Notify, Semaphore};

use pu_transport::{Endpoint, Listener, PeerIdentity, Stream};

const MAX_MESSAGE_SIZE: u64 = 1024 * 1024; // 1MB
/// The app holds a status and a grid stream per open project plus one attach
/// stream per terminal pane, all for as long as they are open, so this must sit
/// well above what a busy session uses.
const MAX_CONNECTIONS: usize = 1024;
const ATTACH_OUTPUT_CHUNK_SIZE: usize = 64 * 1024;
/// How long a new connection may take to send its first complete request.
/// Streams and request loops have no idle limit once a request has arrived.
const FIRST_LINE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

use crate::engine::Engine;
use pu_core::protocol::{Request, Response};

type IpcReader = BufReader<pu_transport::ReadHalf>;
type IpcWriter = pu_transport::WriteHalf;

mod signals;
mod streams;

#[cfg(test)]
mod tests;

// Unread bytes mean the client moved on; otherwise ask the transport, since
// tokio may not yet have registered a readiness event for an already-visible
// EOF. Checked before polling delivery, which can enqueue an OS write.
fn operation_disconnected(reader: &IpcReader) -> bool {
    !reader.buffer().is_empty() || reader.get_ref().peer_sent_or_closed()
}

enum StreamMode {
    None,
    Attach(String, Option<pu_core::protocol::Replay>, Option<String>),
    Grid(String),
    Status(String),
    Projects,
    Fs { root: String, recursive: bool },
}

impl StreamMode {
    fn from_request(request: &Request) -> Self {
        match request {
            Request::Attach {
                agent_id,
                replay,
                client_id,
            } => Self::Attach(agent_id.clone(), *replay, client_id.clone()),
            Request::SubscribeGrid { project_root } => Self::Grid(project_root.clone()),
            Request::SubscribeStatus { project_root } => Self::Status(project_root.clone()),
            Request::SubscribeProjects => Self::Projects,
            Request::SubscribeFs { root, recursive } => Self::Fs {
                root: root.clone(),
                recursive: *recursive,
            },
            _ => Self::None,
        }
    }
}

/// What one bounded read of a newline-delimited request produced.
#[derive(Debug, PartialEq, Eq)]
enum Frame {
    Line(String),
    Eof,
    /// `MAX_MESSAGE_SIZE` bytes without a newline. The rest of the frame is
    /// still unread, so the connection can never be resynchronized.
    TooLarge,
}

/// Reads the next line. `pending` holds a partial line across calls, which
/// makes this cancel-safe inside `select!`: a read interrupted by another
/// branch resumes where it stopped instead of losing bytes. The size limit
/// is applied to raw bytes before UTF-8 decoding, and invalid UTF-8 in a
/// complete line is an `InvalidData` error.
async fn read_frame(reader: &mut IpcReader, pending: &mut Vec<u8>) -> std::io::Result<Frame> {
    let budget = MAX_MESSAGE_SIZE.saturating_sub(pending.len() as u64);
    reader.take(budget).read_until(b'\n', pending).await?;
    // Without a newline, read_until stopped at the size limit or at EOF.
    if !pending.ends_with(b"\n") && pending.len() as u64 >= MAX_MESSAGE_SIZE {
        return Ok(Frame::TooLarge);
    }
    if pending.is_empty() {
        return Ok(Frame::Eof);
    }
    // A complete line, or a final unterminated one at EOF.
    let line = String::from_utf8(std::mem::take(pending))
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    Ok(Frame::Line(line))
}

fn parse_stream_request(line: &str) -> Option<Request> {
    serde_json::from_str(line.trim()).ok()
}

/// Best-effort error for an oversized frame; the caller closes the connection.
async fn reject_frame_too_large(writer: &mut IpcWriter) {
    let resp = Response::Error {
        code: "FRAME_TOO_LARGE".into(),
        message: format!("request exceeds {MAX_MESSAGE_SIZE} bytes without a newline"),
    };
    let _ = write_response(writer, &resp).await;
}

/// Only processes running as the daemon's own user may use the endpoint. The
/// 0600 socket mode or the pipe's owner-only DACL already says so; this holds
/// even if those are changed.
type PeerCheck = fn(&PeerIdentity) -> bool;

pub struct IpcServer {
    listener: Listener,
    engine: Arc<Engine>,
    shutdown: Arc<Notify>,
    conn_limit: Arc<Semaphore>,
    first_line_timeout: std::time::Duration,
    peer_allowed: PeerCheck,
}

impl IpcServer {
    pub fn bind(socket_path: &Path, engine: Engine) -> Result<Self, std::io::Error> {
        // Replaces a stale Unix socket and makes it owner-only whatever the
        // umask; on Windows creates the private first pipe instance.
        // Connections accepted before the mode is set are still held to the
        // peer check in `run`.
        let listener = Listener::bind(&Endpoint::for_socket(socket_path)?)?;
        let engine = Arc::new(engine);
        // Every served registry is pruned, so the projects stream always opens.
        engine.start_registry_prune();
        Ok(Self {
            listener,
            engine,
            shutdown: Arc::new(Notify::new()),
            conn_limit: Arc::new(Semaphore::new(MAX_CONNECTIONS)),
            first_line_timeout: FIRST_LINE_TIMEOUT,
            peer_allowed: PeerIdentity::is_same_user,
        })
    }

    /// Override the concurrent connection cap (defaults to `MAX_CONNECTIONS`).
    #[cfg(test)]
    pub(crate) fn with_connection_limit(mut self, limit: usize) -> Self {
        self.conn_limit = Arc::new(Semaphore::new(limit));
        self
    }

    /// Override how long a connection may wait before its first request.
    #[cfg(test)]
    pub(crate) fn with_first_line_timeout(mut self, timeout: std::time::Duration) -> Self {
        self.first_line_timeout = timeout;
        self
    }

    /// Override the peer check (defaults to [`PeerIdentity::is_same_user`]).
    #[cfg(test)]
    pub(crate) fn with_peer_check(mut self, check: PeerCheck) -> Self {
        self.peer_allowed = check;
        self
    }

    /// Get a reference to the engine for starting background tasks.
    pub fn engine(&self) -> &Arc<Engine> {
        &self.engine
    }

    pub async fn run(mut self) -> Result<(), std::io::Error> {
        let mut signals = signals::ShutdownSignals::new()?;

        loop {
            tokio::select! {
                accept = self.listener.accept() => {
                    // A failed accept (e.g. EMFILE) is transient: never let it end
                    // the server and strand every running agent.
                    let (stream, peer) = match accept {
                        Ok(accepted) => accepted,
                        Err(e) => {
                            tracing::warn!("accept failed: {e}");
                            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                            continue;
                        }
                    };
                    if !(self.peer_allowed)(&peer) {
                        tracing::warn!(peer_pid = ?peer.pid, "rejecting connection from another user");
                        continue; // dropping the stream closes it
                    }
                    // Never wait for a permit: waiting here would stall accepting,
                    // shutdown and signals, and parking the stream in a task would
                    // let stalled clients pile up descriptors past the cap.
                    let Ok(permit) = self.conn_limit.clone().try_acquire_owned() else {
                        tracing::warn!("connection limit reached; rejecting connection");
                        reject_busy(stream);
                        continue;
                    };
                    let engine = self.engine.clone();
                    let shutdown = self.shutdown.clone();
                    let first_line_timeout = self.first_line_timeout;
                    tokio::spawn(async move {
                        let _permit = permit;
                        Self::handle_connection(stream, engine, shutdown, first_line_timeout).await;
                    });
                }
                _ = self.shutdown.notified() => {
                    tracing::info!("shutdown requested via IPC");
                    return Ok(());
                }
                name = signals.recv() => {
                    tracing::info!("received {name}, shutting down");
                    return Ok(());
                }
            }
        }
    }

    async fn handle_connection(
        stream: Stream,
        engine: Arc<Engine>,
        shutdown: Arc<Notify>,
        first_line_timeout: std::time::Duration,
    ) {
        let (reader, writer) = stream.into_split();
        let mut reader = BufReader::new(reader);
        let mut writer = writer;
        let mut pending = Vec::new();

        // A connection that never sends a request would hold a permit forever.
        let first = tokio::time::timeout(first_line_timeout, read_frame(&mut reader, &mut pending));
        let mut frame = match first.await {
            Ok(frame) => frame,
            Err(_) => {
                tracing::debug!("closing connection that sent no request");
                return;
            }
        };
        loop {
            let line = match frame {
                Ok(Frame::Line(line)) => line,
                Ok(Frame::TooLarge) => return reject_frame_too_large(&mut writer).await,
                Ok(Frame::Eof) | Err(_) => return,
            };
            if !Self::dispatch_request(
                &line,
                &mut reader,
                &mut pending,
                &mut writer,
                &engine,
                &shutdown,
            )
            .await
            {
                return;
            }
            frame = read_frame(&mut reader, &mut pending).await;
        }
    }

    /// Handles a single parsed request line. Returns `true` to continue the
    /// connection loop, `false` to break.
    async fn dispatch_request(
        line: &str,
        reader: &mut IpcReader,
        pending: &mut Vec<u8>,
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

        let cancel_on_disconnect = matches!(
            &request,
            Request::Input {
                cancel_on_disconnect: true,
                ..
            }
        );
        if cancel_on_disconnect && operation_disconnected(reader) {
            return false;
        }
        let response = if cancel_on_disconnect {
            tokio::select! {
                biased;
                _ = reader.read_u8() => return false,
                response = engine.handle_prepared_request(request) => response,
            }
        } else {
            engine.handle_prepared_request(request).await
        };
        if !matches!(stream_mode, StreamMode::Attach(..))
            && write_response(writer, &response).await.is_err()
        {
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
            StreamMode::Attach(agent_id, replay, client_id) => {
                if !matches!(response, Response::AttachReady { .. }) {
                    let _ = write_response(writer, &response).await;
                    return false;
                }
                return streams::handle_attach_stream(
                    reader, pending, writer, engine, &agent_id, replay, client_id,
                )
                .await;
            }
            StreamMode::Grid(project_root) => {
                if matches!(response, Response::GridSubscribed) {
                    return streams::handle_grid_stream(
                        reader,
                        pending,
                        writer,
                        engine,
                        &project_root,
                    )
                    .await;
                }
            }
            StreamMode::Status(project_root) => {
                if matches!(response, Response::StatusSubscribed) {
                    return streams::handle_status_stream(
                        reader,
                        pending,
                        writer,
                        engine,
                        &project_root,
                    )
                    .await;
                }
            }
            StreamMode::Projects => {
                if matches!(response, Response::ProjectsSubscribed) {
                    return streams::handle_projects_stream(reader, pending, writer, engine).await;
                }
            }
            StreamMode::Fs { root, recursive } => {
                if matches!(response, Response::FsSubscribed) {
                    // A watch stream never returns to request mode: if the
                    // watcher fails after `FsSubscribed`, closing the
                    // connection is the client's unambiguous resubscribe signal.
                    streams::handle_fs_stream(reader, pending, writer, engine, &root, recursive)
                        .await;
                    return false;
                }
            }
            StreamMode::None => {}
        }

        true
    }
}

/// Best-effort BUSY reply to a connection over the limit, then close it.
///
/// Unix writes without waiting; a partial or failed write is ignored since
/// the stream is closed right after. A freshly connected Windows pipe has no
/// write readiness yet, so `try_write` would drop the line for some clients
/// (WIN-1.2 finding 1): there the line is written by a short-lived task with
/// a 1 s timeout, at most `MAX_BUSY_WRITERS` at a time. Over-cap clients
/// beyond those are closed without a line.
fn reject_busy(stream: Stream) {
    let resp = Response::Error {
        code: "BUSY".into(),
        message: "daemon connection limit reached".into(),
    };
    let Ok(mut json) = serde_json::to_vec(&resp) else {
        return;
    };
    json.push(b'\n');
    #[cfg(unix)]
    {
        let _ = stream.try_write(&json);
    }
    #[cfg(windows)]
    {
        const MAX_BUSY_WRITERS: usize = 128;
        static BUSY_WRITERS: Semaphore = Semaphore::const_new(MAX_BUSY_WRITERS);
        let Ok(permit) = BUSY_WRITERS.try_acquire() else {
            return;
        };
        tokio::spawn(async move {
            let _permit = permit;
            let mut stream = stream;
            let write = stream.write_all(&json);
            let _ = tokio::time::timeout(std::time::Duration::from_secs(1), write).await;
        });
    }
}

async fn write_response(writer: &mut IpcWriter, response: &Response) -> std::io::Result<()> {
    let mut json = serde_json::to_vec(response)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    json.push(b'\n');
    writer.write_all(&json).await
}

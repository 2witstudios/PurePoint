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
/// How long a new connection may take to send its first complete request.
/// Streams and request loops have no idle limit once a request has arrived.
const FIRST_LINE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

use crate::engine::Engine;
use pu_core::protocol::{Request, Response};

type IpcReader = BufReader<tokio::net::unix::OwnedReadHalf>;
type IpcWriter = tokio::net::unix::OwnedWriteHalf;

mod streams;

#[cfg(test)]
mod tests;

// Tokio may not yet have registered a readiness event for an already-visible
// EOF. Check the kernel before polling delivery, which can enqueue an OS write.
fn operation_disconnected(reader: &IpcReader) -> bool {
    use std::os::fd::AsRawFd;
    if !reader.buffer().is_empty() {
        return true;
    }
    let mut byte = 0u8;
    let result = unsafe {
        libc::recv(
            reader.get_ref().as_ref().as_raw_fd(),
            (&mut byte as *mut u8).cast(),
            1,
            libc::MSG_PEEK | libc::MSG_DONTWAIT,
        )
    };
    result >= 0 || std::io::Error::last_os_error().kind() != std::io::ErrorKind::WouldBlock
}

enum StreamMode {
    None,
    Attach(String, Option<pu_core::protocol::Replay>, Option<String>),
    Grid(String),
    Status(String),
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

/// Only processes running as the daemon's own user may use the socket. The
/// 0600 socket mode already says so; this holds even if the mode is changed.
fn peer_allowed(peer_uid: Option<u32>, daemon_euid: u32) -> bool {
    peer_uid == Some(daemon_euid)
}

fn daemon_euid() -> u32 {
    // SAFETY: geteuid has no preconditions and cannot fail.
    unsafe { libc::geteuid() }
}

pub struct IpcServer {
    listener: UnixListener,
    engine: Arc<Engine>,
    shutdown: Arc<Notify>,
    conn_limit: Arc<Semaphore>,
    first_line_timeout: std::time::Duration,
}

impl IpcServer {
    pub fn bind(socket_path: &Path, engine: Engine) -> Result<Self, std::io::Error> {
        use std::os::unix::fs::PermissionsExt;
        // Remove stale socket if it exists
        let _ = std::fs::remove_file(socket_path);
        let listener = UnixListener::bind(socket_path)?;
        // Owner-only whatever the umask. Connections accepted before this are
        // still held to the peer check in `run`.
        std::fs::set_permissions(socket_path, std::fs::Permissions::from_mode(0o600))?;
        Ok(Self {
            listener,
            engine: Arc::new(engine),
            shutdown: Arc::new(Notify::new()),
            conn_limit: Arc::new(Semaphore::new(MAX_CONNECTIONS)),
            first_line_timeout: FIRST_LINE_TIMEOUT,
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

    /// Get a reference to the engine for starting background tasks.
    pub fn engine(&self) -> &Arc<Engine> {
        &self.engine
    }

    pub async fn run(self) -> Result<(), std::io::Error> {
        let mut sigterm =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        let mut sigint = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
        // The terminal or SSH session that started the daemon may hang up; the
        // daemon must outlive it. A handler (unlike SIG_IGN) resets on exec, so
        // agents still get SIGHUP when their own PTY closes.
        let mut sighup = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())?;

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
                    let peer_uid = stream.peer_cred().ok().map(|cred| cred.uid());
                    if !peer_allowed(peer_uid, daemon_euid()) {
                        tracing::warn!(?peer_uid, "rejecting connection from another user");
                        continue; // dropping the stream closes it
                    }
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
                _ = sigterm.recv() => {
                    tracing::info!("received SIGTERM, shutting down");
                    return Ok(());
                }
                _ = sigint.recv() => {
                    tracing::info!("received SIGINT, shutting down");
                    return Ok(());
                }
                _ = sighup.recv() => {
                    tracing::info!("received SIGHUP, ignoring");
                }
            }
        }
    }

    async fn handle_connection(
        stream: tokio::net::UnixStream,
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

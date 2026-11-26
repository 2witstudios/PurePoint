//! Port of `Services/DaemonClient.swift` (with `crates/pu-cli/src/client.rs`): newline-
//! delimited JSON to a daemon.
//!
//! - [`DaemonClient::request`]: one connection, one line out, one line in, under a timeout
//!   (30 s, or the CLI's delivery allowance for requests that type a prompt).
//! - [`DaemonClient::connect`]: a long-lived connection, split into a [`LineReader`] and a
//!   writer, for the attach, status, grid, projects and fs streams (REQ-WINAPP-059).
//!
//! A [`Connector`] opens the byte stream: [`LocalConnector`] is this user's daemon over
//! `pu-transport` (a private named pipe on Windows, verified before any byte is sent);
//! [`super::relay::SshRelayConnector`] is a machine's daemon over `ssh.exe … pu daemon proxy`.
//! Replies parse as [`pu_core::protocol::Response`]; `BUSY` replies and lines that do not
//! parse become typed [`DaemonError`]s.

use std::fmt;
use std::future::Future;
use std::io;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use pu_core::protocol::{Request, Response};
use serde::Serialize;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};

/// One-shot requests time out after 30 s (REQ-WINAPP-059).
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Requests that type a prompt into an agent wait on the daemon's screen-confirmed delivery,
/// up to ~170 s; the same allowance as `pu` (`crates/pu-cli/src/client.rs`), so the daemon's
/// own error reaches the user instead of a bare timeout.
pub const DELIVERY_TIMEOUT: Duration = Duration::from_secs(180);

/// Longest a connect may take before the request counts as failed.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// The timeout `pu` applies to `request`.
pub fn timeout_for(request: &Request) -> Duration {
    match request {
        Request::Input { submit: true, .. } => DELIVERY_TIMEOUT,
        // Plan mode prefixes its own instructions, so even a blank prompt is delivered.
        Request::Spawn {
            prompt, plan_mode, ..
        } if *plan_mode || !prompt.trim().is_empty() => DELIVERY_TIMEOUT,
        _ => REQUEST_TIMEOUT,
    }
}

/// The read half of a daemon connection.
pub type BoxRead = Box<dyn AsyncRead + Send + Unpin>;
/// The write half of a daemon connection.
pub type BoxWrite = Box<dyn AsyncWrite + Send + Unpin>;
/// What [`Connector::open`] returns.
pub type OpenFuture<'a> =
    Pin<Box<dyn Future<Output = Result<(BoxRead, BoxWrite), DaemonError>> + Send + 'a>>;

/// Opens one connection to a daemon. Each call is a new connection, which is what the
/// protocol expects: one per request, plus one per long-lived stream.
pub trait Connector: Send + Sync {
    fn open(&self) -> OpenFuture<'_>;
    /// Where this connector reaches, for logs and errors.
    fn describe(&self) -> String;
}

/// Why a daemon call failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DaemonError {
    /// Nothing listens at the endpoint (no pipe, refused, or the machine is unknown).
    NotRunning(String),
    /// The daemon is alive but at its connection limit (`{"type":"error","code":"BUSY"}`).
    Busy(String),
    /// No reply within the timeout.
    Timeout(Duration),
    /// The daemon closed the connection before a full line.
    Eof,
    /// A reply line that is not a [`Response`].
    Malformed { error: String, preview: String },
    /// The running daemon speaks another protocol version.
    Incompatible { daemon: u32, app: u32 },
    /// No `pu-engine.exe` may be started; names why each candidate was refused.
    BinaryNotFound(String),
    /// A started daemon never answered its health check.
    StartupTimeout,
    /// A request that is not a valid `Request`.
    InvalidRequest(String),
    /// A request the app could not encode, or any other I/O failure.
    Io(String),
}

impl DaemonError {
    /// A stable identifier for the front end.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::NotRunning(_) => "not_running",
            Self::Busy(_) => "busy",
            Self::Timeout(_) => "timeout",
            Self::Eof => "eof",
            Self::Malformed { .. } => "malformed",
            Self::Incompatible { .. } => "incompatible",
            Self::BinaryNotFound(_) => "binary_not_found",
            Self::StartupTimeout => "startup_timeout",
            Self::InvalidRequest(_) => "invalid_request",
            Self::Io(_) => "io",
        }
    }

    fn from_connect(error: io::Error, target: &str) -> Self {
        match error.kind() {
            io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused => {
                Self::NotRunning(format!("Daemon is not running ({target})"))
            }
            _ => Self::Io(format!(
                "Could not connect to the daemon at {target}: {error}"
            )),
        }
    }
}

impl fmt::Display for DaemonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotRunning(message) | Self::Io(message) => f.write_str(message),
            Self::Busy(message) if message.is_empty() => {
                f.write_str("Daemon is at its connection limit")
            }
            Self::Busy(message) => write!(f, "Daemon is at its connection limit: {message}"),
            Self::Timeout(_) => f.write_str("Timed out waiting for the daemon"),
            Self::Eof => f.write_str("Connection to daemon closed"),
            Self::Malformed { error, .. } => write!(f, "Unreadable reply from the daemon: {error}"),
            // The Mac shows a machine's mismatch as "Incompatible (protocol N)"
            // (`MachineState.swift`); the banner adds what to do about it.
            Self::Incompatible { daemon, app } => write!(
                f,
                "Incompatible daemon (protocol {daemon}); this app speaks protocol {app}. Update PurePoint and restart the daemon after saving active agent work."
            ),
            Self::BinaryNotFound(reasons) => write!(
                f,
                "Could not find pu-engine.exe ({reasons}). Reinstall PurePoint or set PU_ENGINE_PATH."
            ),
            Self::StartupTimeout => f.write_str("Daemon did not become healthy within 3 seconds."),
            Self::InvalidRequest(error) => write!(f, "Invalid daemon request: {error}"),
        }
    }
}

impl std::error::Error for DaemonError {}

/// Commands reject with `{ kind, message }`; `message` is what the error banner shows.
impl Serialize for DaemonError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("DaemonError", 2)?;
        s.serialize_field("kind", self.kind())?;
        s.serialize_field("message", &self.to_string())?;
        s.end()
    }
}

/// This user's daemon behind a socket path (`%USERPROFILE%\.pu\daemon.sock` by default).
pub struct LocalConnector {
    endpoint: pu_transport::Endpoint,
}

impl LocalConnector {
    pub fn new(socket: &Path) -> io::Result<Self> {
        Ok(Self {
            endpoint: pu_transport::Endpoint::for_socket(socket)?,
        })
    }

    pub fn endpoint(&self) -> &pu_transport::Endpoint {
        &self.endpoint
    }
}

impl Connector for LocalConnector {
    fn open(&self) -> OpenFuture<'_> {
        Box::pin(async move {
            let stream = pu_transport::connect(&self.endpoint)
                .await
                .map_err(|e| DaemonError::from_connect(e, &self.endpoint.display()))?;
            let (read, write) = stream.into_split();
            Ok((Box::new(read) as BoxRead, Box::new(write) as BoxWrite))
        })
    }

    fn describe(&self) -> String {
        self.endpoint.display()
    }
}

/// The longest reply line read. The largest reply the daemon sends is `git_exec` with its
/// 8 MiB stdout and stderr caps, base64 (about 22 MiB of JSON); a longer line means a broken
/// or hostile peer (a machine's daemon is reached over ssh), and is refused instead of
/// growing the buffer without bound.
pub const MAX_REPLY_LINE: usize = 32 * 1024 * 1024;

/// A line buffer that grew past this is released after its line, so one large reply does
/// not pin its memory for the life of a stream.
const KEEP_LINE_CAPACITY: usize = 1024 * 1024;

/// Reads newline-delimited replies off a long-lived connection (`DaemonLineReader`).
pub struct LineReader {
    reader: BufReader<BoxRead>,
    line: Vec<u8>,
    max_line: usize,
}

impl LineReader {
    pub fn new(read: BoxRead) -> Self {
        Self::with_max_line(read, MAX_REPLY_LINE)
    }

    /// A reader that refuses lines longer than `max_line` bytes.
    pub fn with_max_line(read: BoxRead, max_line: usize) -> Self {
        Self {
            reader: BufReader::new(read),
            line: Vec::new(),
            max_line,
        }
    }

    /// The line buffer's capacity (tests).
    pub fn buffer_capacity(&self) -> usize {
        self.line.capacity()
    }

    /// The next line without its newline, or `None` once the daemon closes the stream.
    /// A final line without a newline is discarded, as on the Mac. A line longer than the
    /// limit is an `InvalidData` error; the stream cannot be read further.
    pub async fn next_line(&mut self) -> io::Result<Option<&[u8]>> {
        if self.line.capacity() > KEEP_LINE_CAPACITY {
            self.line = Vec::new();
        } else {
            self.line.clear();
        }
        loop {
            let available = self.reader.fill_buf().await?;
            if available.is_empty() {
                // Closed, possibly mid-line.
                return Ok(None);
            }
            let (taken, complete) = match available.iter().position(|&b| b == b'\n') {
                Some(end) => (end + 1, true),
                None => (available.len(), false),
            };
            // The newline (and a CR before it) does not count against the limit.
            if self.line.len() + taken > self.max_line + 2 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "the daemon sent a reply longer than {} bytes",
                        self.max_line
                    ),
                ));
            }
            self.line.extend_from_slice(&available[..taken]);
            self.reader.consume(taken);
            if complete {
                break;
            }
        }
        self.line.pop();
        if self.line.last() == Some(&b'\r') {
            self.line.pop();
        }
        Ok(Some(&self.line))
    }

    /// The next reply. A closed stream is [`DaemonError::Eof`].
    pub async fn next_response(&mut self) -> Result<Response, DaemonError> {
        match self.next_line().await {
            Ok(Some(line)) => parse(line),
            Ok(None) => Err(DaemonError::Eof),
            Err(e) => Err(DaemonError::Io(format!(
                "Reading from the daemon failed: {e}"
            ))),
        }
    }
}

/// A reply line as a [`Response`]: `BUSY` and unparseable lines become errors; every other
/// `error` reply is returned for the caller to handle, as the Mac does.
pub fn parse(line: &[u8]) -> Result<Response, DaemonError> {
    match serde_json::from_slice::<Response>(line) {
        Ok(Response::Error { code, message }) if code == "BUSY" => Err(DaemonError::Busy(message)),
        Ok(response) => Ok(response),
        Err(error) => {
            let end = line.len().min(200);
            Err(DaemonError::Malformed {
                error: error.to_string(),
                preview: String::from_utf8_lossy(&line[..end]).into_owned(),
            })
        }
    }
}

/// Write one request line.
pub async fn write_request(writer: &mut BoxWrite, request: &Request) -> Result<(), DaemonError> {
    let mut line = serde_json::to_vec(request)
        .map_err(|e| DaemonError::Io(format!("Could not encode the request: {e}")))?;
    line.push(b'\n');
    writer
        .write_all(&line)
        .await
        .map_err(|e| DaemonError::Io(format!("Sending to the daemon failed: {e}")))?;
    writer
        .flush()
        .await
        .map_err(|e| DaemonError::Io(format!("Sending to the daemon failed: {e}")))
}

/// A client for one daemon. Cheap to clone; every call opens its own connection.
#[derive(Clone)]
pub struct DaemonClient {
    connector: Arc<dyn Connector>,
}

impl DaemonClient {
    pub fn new(connector: Arc<dyn Connector>) -> Self {
        Self { connector }
    }

    /// This user's daemon behind `socket`.
    pub fn local(socket: &Path) -> io::Result<Self> {
        Ok(Self::new(Arc::new(LocalConnector::new(socket)?)))
    }

    pub fn describe(&self) -> String {
        self.connector.describe()
    }

    /// Send one request and read its reply, under [`timeout_for`] the request.
    pub async fn request(&self, request: &Request) -> Result<Response, DaemonError> {
        self.request_with_timeout(request, timeout_for(request))
            .await
    }

    /// Send one request and read its reply. The connect, write and read all count against
    /// `timeout`; a connect alone may take at most [`CONNECT_TIMEOUT`].
    pub async fn request_with_timeout(
        &self,
        request: &Request,
        timeout: Duration,
    ) -> Result<Response, DaemonError> {
        let exchange = async {
            let (read, mut write) = self.open().await?;
            write_request(&mut write, request).await?;
            LineReader::new(read).next_response().await
        };
        tokio::time::timeout(timeout, exchange)
            .await
            .unwrap_or(Err(DaemonError::Timeout(timeout)))
    }

    /// A long-lived connection: a line reader and the write half.
    pub async fn connect(&self) -> Result<(LineReader, BoxWrite), DaemonError> {
        let (read, write) = self.open().await?;
        Ok((LineReader::new(read), write))
    }

    async fn open(&self) -> Result<(BoxRead, BoxWrite), DaemonError> {
        tokio::time::timeout(CONNECT_TIMEOUT, self.connector.open())
            .await
            .unwrap_or_else(|_| {
                Err(DaemonError::Io(format!(
                    "Timed out connecting to the daemon at {}",
                    self.connector.describe()
                )))
            })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use pu_core::protocol::PROTOCOL_VERSION;
    use tokio::io::AsyncReadExt;

    /// How the fake daemon answers each request line.
    #[derive(Clone)]
    pub(crate) enum Reply {
        Line(String),
        Silent,
        Close,
    }

    pub(crate) fn health_line(pid: u32, protocol_version: u32) -> String {
        serde_json::to_string(&Response::HealthReport {
            pid,
            uptime_seconds: 1,
            protocol_version,
            projects: vec![],
            agent_count: 0,
        })
        .unwrap()
    }

    /// An in-process daemon on `socket` that answers every connection's first line with
    /// `reply` (a `pu_transport::Listener`, so the client's server check runs for real).
    pub(crate) fn fake_daemon(socket: &Path, reply: Reply) -> tokio::task::JoinHandle<()> {
        let endpoint = pu_transport::Endpoint::for_socket(socket).unwrap();
        let mut listener = pu_transport::Listener::bind(&endpoint).unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((stream, _peer)) = listener.accept().await else {
                    continue;
                };
                let reply = reply.clone();
                tokio::spawn(async move {
                    let (mut read, mut write) = stream.into_split();
                    let mut got = Vec::new();
                    let mut byte = [0u8; 1];
                    while read.read(&mut byte).await.unwrap_or(0) == 1 && byte[0] != b'\n' {
                        got.push(byte[0]);
                    }
                    match reply {
                        Reply::Line(line) => {
                            let _ = write.write_all(format!("{line}\n").as_bytes()).await;
                            let _ = write.flush().await;
                            tokio::time::sleep(Duration::from_millis(200)).await;
                        }
                        Reply::Silent => tokio::time::sleep(Duration::from_secs(30)).await,
                        Reply::Close => {}
                    }
                });
            }
        })
    }

    fn client(socket: &Path) -> DaemonClient {
        DaemonClient::local(socket).unwrap()
    }

    #[tokio::test]
    async fn given_healthy_daemon_should_parse_its_health_report() {
        let tmp = tempfile::TempDir::new().unwrap();
        let socket = tmp.path().join("d.sock");
        let server = fake_daemon(&socket, Reply::Line(health_line(42, PROTOCOL_VERSION)));
        let response = client(&socket).request(&Request::Health).await.unwrap();
        assert!(
            matches!(response, Response::HealthReport { pid: 42, .. }),
            "{response:?}"
        );
        server.abort();
    }

    #[tokio::test]
    async fn given_silent_daemon_should_time_out() {
        let tmp = tempfile::TempDir::new().unwrap();
        let socket = tmp.path().join("d.sock");
        let server = fake_daemon(&socket, Reply::Silent);
        let timeout = Duration::from_millis(300);
        let result = client(&socket)
            .request_with_timeout(&Request::Health, timeout)
            .await;
        assert!(
            matches!(result, Err(DaemonError::Timeout(t)) if t == timeout),
            "{result:?}"
        );
        server.abort();
    }

    #[tokio::test]
    async fn given_busy_daemon_should_return_busy() {
        let tmp = tempfile::TempDir::new().unwrap();
        let socket = tmp.path().join("d.sock");
        let busy = r#"{"type":"error","code":"BUSY","message":"too many connections"}"#;
        let server = fake_daemon(&socket, Reply::Line(busy.into()));
        let result = client(&socket).request(&Request::Health).await;
        assert!(
            matches!(&result, Err(DaemonError::Busy(m)) if m == "too many connections"),
            "{result:?}"
        );
        server.abort();
    }

    #[tokio::test]
    async fn given_other_error_reply_should_return_it() {
        let tmp = tempfile::TempDir::new().unwrap();
        let socket = tmp.path().join("d.sock");
        let line = r#"{"type":"error","code":"NOT_FOUND","message":"no agent"}"#;
        let server = fake_daemon(&socket, Reply::Line(line.into()));
        let result = client(&socket).request(&Request::Health).await;
        assert!(matches!(result, Ok(Response::Error { code, .. }) if code == "NOT_FOUND"));
        server.abort();
    }

    #[tokio::test]
    async fn given_malformed_line_should_return_malformed() {
        let tmp = tempfile::TempDir::new().unwrap();
        let socket = tmp.path().join("d.sock");
        let server = fake_daemon(&socket, Reply::Line("{not json".into()));
        let result = client(&socket).request(&Request::Health).await;
        match result {
            Err(DaemonError::Malformed { preview, .. }) => assert_eq!(preview, "{not json"),
            other => panic!("expected Malformed, got {other:?}"),
        }
        server.abort();
    }

    #[tokio::test]
    async fn given_daemon_closing_without_reply_should_return_eof() {
        let tmp = tempfile::TempDir::new().unwrap();
        let socket = tmp.path().join("d.sock");
        let server = fake_daemon(&socket, Reply::Close);
        let result = client(&socket).request(&Request::Health).await;
        assert!(matches!(result, Err(DaemonError::Eof)), "{result:?}");
        server.abort();
    }

    #[tokio::test]
    async fn given_no_daemon_should_return_not_running() {
        let tmp = tempfile::TempDir::new().unwrap();
        let result = client(&tmp.path().join("none.sock"))
            .request(&Request::Health)
            .await;
        assert!(
            matches!(result, Err(DaemonError::NotRunning(_))),
            "{result:?}"
        );
    }

    #[tokio::test]
    async fn given_long_lived_connection_should_read_replies_line_by_line() {
        let tmp = tempfile::TempDir::new().unwrap();
        let socket = tmp.path().join("d.sock");
        let server = fake_daemon(&socket, Reply::Line(health_line(7, PROTOCOL_VERSION)));
        let (mut reader, mut writer) = client(&socket).connect().await.unwrap();
        write_request(&mut writer, &Request::Health).await.unwrap();
        assert!(matches!(
            reader.next_response().await,
            Ok(Response::HealthReport { pid: 7, .. })
        ));
        assert!(matches!(
            reader.next_response().await,
            Err(DaemonError::Eof)
        ));
        server.abort();
    }

    #[test]
    fn given_errors_should_serialize_kind_and_banner_message() {
        let value = serde_json::to_value(DaemonError::Incompatible { daemon: 3, app: 14 }).unwrap();
        assert_eq!(value["kind"], "incompatible");
        assert_eq!(
            value["message"],
            "Incompatible daemon (protocol 3); this app speaks protocol 14. Update PurePoint and restart the daemon after saving active agent work."
        );
    }

    #[test]
    fn given_prompted_requests_should_wait_for_delivery() {
        let input = |submit| Request::Input {
            agent_id: "ag-1".into(),
            data: b"hi".to_vec(),
            submit,
            cancel_on_disconnect: false,
        };
        assert_eq!(timeout_for(&input(true)), DELIVERY_TIMEOUT);
        assert_eq!(timeout_for(&input(false)), REQUEST_TIMEOUT);
        assert_eq!(timeout_for(&Request::Health), REQUEST_TIMEOUT);
    }

    fn reader_over(bytes: Vec<u8>, max_line: usize) -> LineReader {
        LineReader::with_max_line(Box::new(std::io::Cursor::new(bytes)), max_line)
    }

    #[tokio::test]
    async fn given_a_line_over_the_limit_should_refuse_it() {
        let mut bytes = b"short\r\n".to_vec();
        bytes.extend(std::iter::repeat_n(b'x', 100));
        bytes.extend(b"\nnever\n");
        let mut reader = reader_over(bytes, 64);
        assert_eq!(reader.next_line().await.unwrap(), Some(&b"short"[..]));
        let error = reader.next_line().await.unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[tokio::test]
    async fn given_a_line_at_the_limit_should_read_it() {
        let mut bytes = vec![b'y'; 64];
        bytes.extend(b"\r\n");
        let mut reader = reader_over(bytes, 64);
        assert_eq!(reader.next_line().await.unwrap().map(<[u8]>::len), Some(64));
        assert_eq!(reader.next_line().await.unwrap(), None);
    }

    #[tokio::test]
    async fn given_a_large_line_should_release_its_buffer_after() {
        let mut bytes = vec![b'z'; 3 * 1024 * 1024];
        bytes.extend(b"\nsmall\n");
        let mut reader = reader_over(bytes, MAX_REPLY_LINE);
        assert_eq!(
            reader.next_line().await.unwrap().map(<[u8]>::len),
            Some(3 * 1024 * 1024)
        );
        assert!(reader.buffer_capacity() > KEEP_LINE_CAPACITY);
        assert_eq!(reader.next_line().await.unwrap(), Some(&b"small"[..]));
        assert!(reader.buffer_capacity() <= KEEP_LINE_CAPACITY);
    }

    #[tokio::test]
    async fn given_a_final_line_without_newline_should_discard_it() {
        let mut reader = reader_over(b"one\ntwo".to_vec(), 64);
        assert_eq!(reader.next_line().await.unwrap(), Some(&b"one"[..]));
        assert_eq!(reader.next_line().await.unwrap(), None);
    }
}

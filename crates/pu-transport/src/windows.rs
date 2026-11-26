//! Private named pipes (WIN-1.2, `docs/research/windows-ipc.md`).
use std::io;
use std::os::windows::io::AsRawHandle;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, ready};
use std::time::{Duration, Instant};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeClient, NamedPipeServer};

use crate::win_sys::{self, PeerState, PipeSecurity, Sid};
use crate::{Endpoint, PeerIdentity};

const ERROR_ACCESS_DENIED: i32 = 5;
const ERROR_PIPE_BUSY: i32 = 231;
/// How long `connect` waits for a free instance before giving up.
const BUSY_WAIT: Duration = Duration::from_secs(2);

enum Pipe {
    Server(NamedPipeServer),
    Client(NamedPipeClient),
}

impl Pipe {
    fn poll_read_ready(&self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self {
            Pipe::Server(p) => p.poll_read_ready(cx),
            Pipe::Client(p) => p.poll_read_ready(cx),
        }
    }
    fn poll_write_ready(&self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self {
            Pipe::Server(p) => p.poll_write_ready(cx),
            Pipe::Client(p) => p.poll_write_ready(cx),
        }
    }
    fn try_read(&self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Pipe::Server(p) => p.try_read(buf),
            Pipe::Client(p) => p.try_read(buf),
        }
    }
    fn try_write(&self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Pipe::Server(p) => p.try_write(buf),
            Pipe::Client(p) => p.try_write(buf),
        }
    }
    fn raw(&self) -> std::os::windows::io::RawHandle {
        match self {
            Pipe::Server(p) => p.as_raw_handle(),
            Pipe::Client(p) => p.as_raw_handle(),
        }
    }
}

/// Bytes (or EOF) pulled forward by a disconnect probe, served to the next read.
#[derive(Default)]
struct Stash {
    bytes: Vec<u8>,
    eof: bool,
}

struct Shared {
    pipe: Pipe,
    stash: Mutex<Stash>,
}

pub struct Stream(Arc<Shared>);
pub struct ReadHalf(Arc<Shared>);
pub struct WriteHalf(Arc<Shared>);

impl Stream {
    fn new(pipe: Pipe) -> Self {
        Stream(Arc::new(Shared {
            pipe,
            stash: Mutex::new(Stash::default()),
        }))
    }
    pub fn into_split(self) -> (ReadHalf, WriteHalf) {
        (ReadHalf(self.0.clone()), WriteHalf(self.0))
    }
    pub fn try_write(&self, buf: &[u8]) -> io::Result<usize> {
        self.0.pipe.try_write(buf)
    }
}

impl ReadHalf {
    pub fn peer_sent_or_closed(&self) -> bool {
        self.0.peer_sent_or_closed()
    }
}

impl WriteHalf {
    pub fn try_write(&self, buf: &[u8]) -> io::Result<usize> {
        self.0.pipe.try_write(buf)
    }
}

impl Shared {
    fn peer_sent_or_closed(&self) -> bool {
        let mut stash = self.stash.lock().unwrap_or_else(|e| e.into_inner());
        if stash.eof || !stash.bytes.is_empty() {
            return true;
        }
        match win_sys::peek(self.pipe.raw()) {
            PeerState::Gone(_) | PeerState::Pending(_) => return true,
            PeerState::Idle => {}
        }
        // Tokio reads ahead into its own buffer, which PeekNamedPipe cannot
        // see: pull anything already there into the stash.
        let mut buf = [0u8; 4096];
        match self.pipe.try_read(&mut buf) {
            Ok(0) => {
                stash.eof = true;
                true
            }
            Ok(n) => {
                stash.bytes.extend_from_slice(&buf[..n]);
                true
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => false,
            Err(_) => true,
        }
    }

    fn poll_read(&self, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        {
            let mut stash = self.stash.lock().unwrap_or_else(|e| e.into_inner());
            if !stash.bytes.is_empty() {
                let n = stash.bytes.len().min(buf.remaining());
                buf.put_slice(&stash.bytes[..n]);
                stash.bytes.drain(..n);
                return Poll::Ready(Ok(()));
            }
            if stash.eof {
                return Poll::Ready(Ok(()));
            }
        }
        loop {
            ready!(self.pipe.poll_read_ready(cx))?;
            match self.pipe.try_read(buf.initialize_unfilled()) {
                Ok(n) => {
                    buf.advance(n);
                    return Poll::Ready(Ok(()));
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => continue,
                // A closed peer is end-of-stream, as on a socket.
                Err(e) if e.raw_os_error() == Some(109) => return Poll::Ready(Ok(())),
                Err(e) => return Poll::Ready(Err(e)),
            }
        }
    }

    fn poll_write(&self, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        loop {
            ready!(self.pipe.poll_write_ready(cx))?;
            match self.pipe.try_write(buf) {
                Ok(n) => return Poll::Ready(Ok(n)),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => continue,
                Err(e) => return Poll::Ready(Err(e)),
            }
        }
    }
}

macro_rules! pipe_io {
    ($t:ty) => {
        impl AsyncRead for $t {
            fn poll_read(
                self: Pin<&mut Self>,
                cx: &mut Context<'_>,
                buf: &mut ReadBuf<'_>,
            ) -> Poll<io::Result<()>> {
                self.0.poll_read(cx, buf)
            }
        }
        impl AsyncWrite for $t {
            fn poll_write(
                self: Pin<&mut Self>,
                cx: &mut Context<'_>,
                buf: &[u8],
            ) -> Poll<io::Result<usize>> {
                self.0.poll_write(cx, buf)
            }
            fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
                Poll::Ready(Ok(()))
            }
            // Pipes have no half-close; the connection ends when both halves drop.
            fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
                Poll::Ready(Ok(()))
            }
        }
    };
}
pipe_io!(Stream);
pipe_io!(ReadHalf);
pipe_io!(WriteHalf);

/// Verify who owns the pipe before sending anything (pipe names are global).
fn verify_server(client: &NamedPipeClient, endpoint: &Endpoint, me: &Sid) -> io::Result<()> {
    let pid = win_sys::pipe_server_pid(client.as_raw_handle())?;
    let sid = win_sys::process_user_sid(pid)?;
    if &sid != me {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "refusing {}: served by another user ({sid}, pid {pid})",
                endpoint.display()
            ),
        ));
    }
    // A same-user impostor passes the SID check; the daemon's pid file
    // names the real server. A pid file whose process is gone is stale.
    if let Some(expected) = std::fs::read_to_string(endpoint.pid_path())
        .ok()
        .and_then(|t| t.trim().parse::<u32>().ok())
        && expected != pid
        && win_sys::process_alive(expected)
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "refusing {}: served by pid {pid}, but the daemon is pid {expected}",
                endpoint.display()
            ),
        ));
    }
    Ok(())
}

pub async fn connect(endpoint: &Endpoint) -> io::Result<Stream> {
    let me = win_sys::current_user_sid()?;
    let deadline = Instant::now() + BUSY_WAIT;
    let client = loop {
        match ClientOptions::new().open(endpoint.pipe_name()) {
            Ok(c) => break c,
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY) && Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            Err(e) => return Err(e),
        }
    };
    verify_server(&client, endpoint, &me)?;
    Ok(Stream::new(Pipe::Client(client)))
}

pub struct Listener {
    name: String,
    security: PipeSecurity,
    me: Sid,
    next: NamedPipeServer,
}

impl Listener {
    pub fn bind(endpoint: &Endpoint) -> io::Result<Self> {
        let security = PipeSecurity::current_user()?;
        let next = security.create(endpoint.pipe_name(), true).map_err(|e| {
            if e.raw_os_error() == Some(ERROR_ACCESS_DENIED) {
                io::Error::new(
                    io::ErrorKind::AddrInUse,
                    format!("{} is already owned by another process", endpoint.display()),
                )
            } else {
                e
            }
        })?;
        Ok(Self {
            name: endpoint.pipe_name().to_owned(),
            security,
            me: win_sys::current_user_sid()?,
            next,
        })
    }

    pub async fn accept(&mut self) -> io::Result<(Stream, PeerIdentity)> {
        let result = self.next.connect().await;
        // Always have a listening instance before handing one off: with none,
        // clients see "not found" instead of a retryable busy pipe.
        let fresh = self.security.create(&self.name, false)?;
        let connected = std::mem::replace(&mut self.next, fresh);
        result?;
        let pid = win_sys::pipe_client_pid(connected.as_raw_handle()).ok();
        let same_user = pid
            .and_then(|p| win_sys::process_user_sid(p).ok())
            .is_some_and(|sid| sid == self.me);
        Ok((
            Stream::new(Pipe::Server(connected)),
            PeerIdentity { pid, same_user },
        ))
    }
}

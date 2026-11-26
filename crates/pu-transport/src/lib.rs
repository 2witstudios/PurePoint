//! Daemon IPC endpoints for every platform.
//!
//! The daemon protocol (newline-delimited JSON, see `docs/reference/ipc-api.md`)
//! is transport-agnostic. This crate supplies the byte streams:
//!
//! - **Unix**: a Unix domain socket at the socket path (`~/.pu/daemon.sock`),
//!   mode 0600, peers checked by uid.
//! - **Windows**: a named pipe whose name is derived from the normalized socket
//!   path (DEC-2), created with an owner-only DACL, peers checked by user SID,
//!   and servers verified by clients (user SID, plus the PID recorded in the
//!   socket's `.pid` file) because pipe names are global.
//!
//! The socket path stays the stem of the pid, lock and `.projects.json` files
//! on every platform; only the stream changes.

use std::io;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

#[cfg(unix)]
mod unix;
#[cfg(unix)]
use unix as imp;

#[cfg(windows)]
pub mod naming;
#[cfg(windows)]
pub mod signal;
#[cfg(windows)]
mod win_sys;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as imp;

/// Where a daemon listens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    socket_path: PathBuf,
    #[cfg(windows)]
    pipe_name: String,
}

impl Endpoint {
    /// The endpoint for a daemon whose socket path is `socket_path`.
    ///
    /// On Windows the pipe name hashes the socket's canonical path; this
    /// never creates anything (see `naming`); only [`Listener::bind`] does.
    pub fn for_socket(socket_path: &Path) -> io::Result<Self> {
        Ok(Self {
            socket_path: socket_path.to_path_buf(),
            #[cfg(windows)]
            pipe_name: naming::pipe_name_for_socket(socket_path)?,
        })
    }

    /// The socket path this endpoint was derived from.
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// The `<socket>.pid` file the daemon writes.
    pub fn pid_path(&self) -> PathBuf {
        let mut p = self.socket_path.as_os_str().to_os_string();
        p.push(".pid");
        PathBuf::from(p)
    }

    /// The Windows pipe name (`\\.\pipe\purepoint-<hash>-daemon`).
    #[cfg(windows)]
    pub fn pipe_name(&self) -> &str {
        &self.pipe_name
    }

    /// Human-readable address for logs and errors.
    pub fn display(&self) -> String {
        #[cfg(windows)]
        return format!("{} ({})", self.pipe_name, self.socket_path.display());
        #[cfg(not(windows))]
        return self.socket_path.display().to_string();
    }
}

/// The endpoint of the user's default daemon (`~/.pu/daemon.sock`).
pub fn daemon_endpoint() -> io::Result<Endpoint> {
    Endpoint::for_socket(&pu_core::paths::daemon_socket_path()?)
}

/// Who is on the other end of an accepted connection.
#[derive(Debug, Clone)]
pub struct PeerIdentity {
    /// The peer's process id, when the platform reports it.
    pub pid: Option<u32>,
    same_user: bool,
}

impl PeerIdentity {
    /// True when the peer runs as the same user as this process (Unix uid,
    /// Windows user SID). Unknown identities are never the same user.
    pub fn is_same_user(&self) -> bool {
        self.same_user
    }
}

/// The Unix peer rule: a peer is the daemon's own user only when its uid is
/// known and equals the daemon's effective uid. Root peers are not exempt.
#[cfg_attr(not(unix), allow(dead_code))]
pub(crate) fn same_user(peer_uid: Option<u32>, euid: u32) -> bool {
    peer_uid == Some(euid)
}

/// Who serves the client end `pipe` of a named pipe: its process id and
/// whether it runs as this user. Pipe names are global, so a client checks
/// this before sending a credential to a pipe it did not create.
#[cfg(windows)]
pub fn pipe_server_identity(pipe: std::os::windows::io::RawHandle) -> io::Result<PeerIdentity> {
    let pid = win_sys::pipe_server_pid(pipe)?;
    let same_user = win_sys::process_user_sid(pid)? == win_sys::current_user_sid()?;
    Ok(PeerIdentity {
        pid: Some(pid),
        same_user,
    })
}

/// A connected duplex byte stream to or from a daemon.
pub struct Stream(imp::Stream);

/// Read half of a [`Stream`].
pub struct ReadHalf(imp::ReadHalf);

/// Write half of a [`Stream`].
pub struct WriteHalf(imp::WriteHalf);

impl Stream {
    /// Split into independently owned halves.
    pub fn into_split(self) -> (ReadHalf, WriteHalf) {
        let (r, w) = self.0.into_split();
        (ReadHalf(r), WriteHalf(w))
    }

    /// Write without waiting; `WouldBlock` when the peer is not ready.
    pub fn try_write(&self, buf: &[u8]) -> io::Result<usize> {
        self.0.try_write(buf)
    }
}

impl ReadHalf {
    /// True when the peer has sent more bytes or hung up, without consuming
    /// anything: the Unix `recv(MSG_PEEK)` check, `PeekNamedPipe` plus a
    /// readiness probe on Windows.
    pub fn peer_sent_or_closed(&self) -> bool {
        self.0.peer_sent_or_closed()
    }
}

impl WriteHalf {
    /// Write without waiting; `WouldBlock` when the peer is not ready.
    pub fn try_write(&self, buf: &[u8]) -> io::Result<usize> {
        self.0.try_write(buf)
    }
}

/// Connect to a running daemon. Never starts one.
///
/// On Windows the server is verified before any byte is sent: its user SID
/// must equal ours and, when the endpoint's `.pid` file names a live
/// process, its PID must match.
pub async fn connect(endpoint: &Endpoint) -> io::Result<Stream> {
    imp::connect(endpoint).await.map(Stream)
}

/// Accepts daemon connections.
pub struct Listener(imp::Listener);

impl Listener {
    /// Bind the endpoint, replacing a stale Unix socket file. On Windows,
    /// fails with `AddrInUse` if another process already owns the pipe name.
    pub fn bind(endpoint: &Endpoint) -> io::Result<Self> {
        imp::Listener::bind(endpoint).map(Listener)
    }

    /// Wait for the next client. Transient failures are returned as errors;
    /// callers keep accepting.
    pub async fn accept(&mut self) -> io::Result<(Stream, PeerIdentity)> {
        let (s, p) = self.0.accept().await?;
        Ok((Stream(s), p))
    }
}

macro_rules! delegate_read {
    ($t:ty) => {
        impl AsyncRead for $t {
            fn poll_read(
                mut self: Pin<&mut Self>,
                cx: &mut Context<'_>,
                buf: &mut ReadBuf<'_>,
            ) -> Poll<io::Result<()>> {
                Pin::new(&mut self.0).poll_read(cx, buf)
            }
        }
    };
}
macro_rules! delegate_write {
    ($t:ty) => {
        impl AsyncWrite for $t {
            fn poll_write(
                mut self: Pin<&mut Self>,
                cx: &mut Context<'_>,
                buf: &[u8],
            ) -> Poll<io::Result<usize>> {
                Pin::new(&mut self.0).poll_write(cx, buf)
            }
            fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
                Pin::new(&mut self.0).poll_flush(cx)
            }
            fn poll_shutdown(
                mut self: Pin<&mut Self>,
                cx: &mut Context<'_>,
            ) -> Poll<io::Result<()>> {
                Pin::new(&mut self.0).poll_shutdown(cx)
            }
        }
    };
}
delegate_read!(Stream);
delegate_write!(Stream);
delegate_read!(ReadHalf);
delegate_write!(WriteHalf);

#[cfg(test)]
mod tests;

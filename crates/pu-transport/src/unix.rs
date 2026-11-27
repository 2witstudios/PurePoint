//! Unix domain sockets: the daemon's original transport, unchanged.
use std::io;
use std::os::fd::AsRawFd;
use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{UnixListener, UnixStream};

use crate::{Endpoint, PeerIdentity};

pub struct Stream(UnixStream);
pub struct ReadHalf(OwnedReadHalf);
pub struct WriteHalf(OwnedWriteHalf);

fn euid() -> u32 {
    // SAFETY: geteuid has no preconditions and cannot fail.
    unsafe { libc::geteuid() }
}

impl Stream {
    pub fn into_split(self) -> (ReadHalf, WriteHalf) {
        let (r, w) = self.0.into_split();
        (ReadHalf(r), WriteHalf(w))
    }
    pub fn try_write(&self, buf: &[u8]) -> io::Result<usize> {
        self.0.try_write(buf)
    }
}

impl ReadHalf {
    // Tokio may not yet have registered a readiness event for an
    // already-visible EOF, so ask the kernel.
    pub fn peer_sent_or_closed(&self) -> bool {
        let mut byte = 0u8;
        // SAFETY: valid fd, one-byte buffer, non-blocking peek.
        let result = unsafe {
            libc::recv(
                self.0.as_ref().as_raw_fd(),
                (&mut byte as *mut u8).cast(),
                1,
                libc::MSG_PEEK | libc::MSG_DONTWAIT,
            )
        };
        result >= 0 || io::Error::last_os_error().kind() != io::ErrorKind::WouldBlock
    }
}

impl WriteHalf {
    pub fn try_write(&self, buf: &[u8]) -> io::Result<usize> {
        self.0.try_write(buf)
    }
}

pub async fn connect(endpoint: &Endpoint) -> io::Result<Stream> {
    UnixStream::connect(endpoint.socket_path())
        .await
        .map(Stream)
}

pub struct Listener(UnixListener);

impl Listener {
    pub fn bind(endpoint: &Endpoint) -> io::Result<Self> {
        use std::os::unix::fs::PermissionsExt;
        let path = endpoint.socket_path();
        // Remove a stale socket if one exists.
        let _ = std::fs::remove_file(path);
        let listener = UnixListener::bind(path)?;
        // Owner-only whatever the umask. Connections accepted before this are
        // still held to the peer check in `accept`.
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        Ok(Self(listener))
    }

    pub async fn accept(&mut self) -> io::Result<(Stream, PeerIdentity)> {
        let (stream, _addr) = self.0.accept().await?;
        let cred = stream.peer_cred().ok();
        let peer = PeerIdentity {
            pid: cred
                .as_ref()
                .and_then(|c| c.pid())
                .and_then(|p| u32::try_from(p).ok()),
            same_user: crate::same_user(cred.map(|c| c.uid()), euid()),
        };
        Ok((Stream(stream), peer))
    }
}

macro_rules! fwd_read {
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
macro_rules! fwd_write {
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
fwd_read!(Stream);
fwd_write!(Stream);
fwd_read!(ReadHalf);
fwd_write!(WriteHalf);

//! Per-launch agent signal pipe: the Windows replacement for the launch's
//! `UnixDatagram` (WIN-1.2 "Signal pipe", REQ-WINAPP-047/048).
//!
//! - Name: `\\.\pipe\purepoint-<hash16>-signal-<nonce>`, the daemon socket's
//!   hash plus a per-launch nonce, created with `FILE_FLAG_FIRST_PIPE_INSTANCE`
//!   so nobody else can own it, and the daemon pipe's owner-only DACL.
//! - Protocol: one frame (`{"token","payload"}` JSON) per connection, ended by
//!   the writer closing, at most [`MAX_FRAME`] bytes, read within one second.
//!   The token is checked by the caller in constant time.
//! - Frames are delivered in connection order.
//! - Writers must run as the daemon's user: checked from the client process
//!   token, or (when a fast writer already exited) by impersonating the
//!   connection's captured security context.
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use std::time::{Duration, Instant};

use tokio::io::AsyncReadExt;
use tokio::net::windows::named_pipe::NamedPipeServer;
use tokio::sync::{Mutex, mpsc};
use tokio::task::JoinHandle;

use crate::naming;
use crate::win_sys::{self, PipeSecurity, Sid};

/// Largest frame accepted, as on the Unix datagram socket.
pub const MAX_FRAME: usize = 256 * 1024;
const READ_TIMEOUT: Duration = Duration::from_secs(1);
/// Frames buffered for a slow consumer before new ones are dropped.
const QUEUE: usize = 64;
const ERROR_PIPE_BUSY: i32 = 231;

/// `\\.\pipe\purepoint-<hash16>-signal-<nonce>` for the daemon whose socket
/// path is `socket_path`.
pub fn pipe_name(socket_path: &Path, nonce: &str) -> io::Result<String> {
    let normalized = naming::normalize_socket_path(socket_path)?;
    Ok(naming::pipe_name(&normalized, &format!("signal-{nonce}")))
}

/// The server end of one launch's signal pipe. Dropping it closes the pipe.
pub struct SignalListener {
    name: String,
    frames: Mutex<mpsc::Receiver<Vec<u8>>>,
    task: JoinHandle<()>,
}

impl SignalListener {
    /// Create the first instance of `name` (failing if anyone owns it) and
    /// start accepting writers. Must be called inside a Tokio runtime.
    pub fn bind(name: &str) -> io::Result<Self> {
        let security = PipeSecurity::current_user()?;
        let first = security.create(name, true)?;
        let me = win_sys::current_user_sid()?;
        let (tx, rx) = mpsc::channel(QUEUE);
        let task = tokio::spawn(serve(name.to_owned(), security, me, first, tx));
        Ok(Self {
            name: name.to_owned(),
            frames: Mutex::new(rx),
            task,
        })
    }

    /// The pipe name writers open (exported as `PU_SIGNAL_SOCKET`).
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The next frame, copied into `buf` (like `UnixDatagram::recv`; frames
    /// longer than `buf` are truncated, so pass at least [`MAX_FRAME`] bytes).
    pub async fn recv(&self, buf: &mut [u8]) -> io::Result<usize> {
        let frame = self
            .frames
            .lock()
            .await
            .recv()
            .await
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "signal pipe closed"))?;
        let n = frame.len().min(buf.len());
        buf[..n].copy_from_slice(&frame[..n]);
        Ok(n)
    }
}

impl Drop for SignalListener {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn serve(
    name: String,
    security: PipeSecurity,
    me: Sid,
    mut listening: NamedPipeServer,
    tx: mpsc::Sender<Vec<u8>>,
) {
    loop {
        let connected = listening.connect().await;
        // Keep a listening instance before handling this one, so a writer
        // arriving meanwhile waits on PIPE_BUSY instead of seeing no pipe.
        let next = match security.create(&name, false) {
            Ok(next) => next,
            Err(_) => return,
        };
        let pipe = std::mem::replace(&mut listening, next);
        if connected.is_err() {
            continue;
        }
        let early = client_process_sid(&pipe).map(|sid| sid == me);
        if early == Some(false) {
            continue;
        }
        // Read connections in accept order, as a datagram socket delivers
        // them: hook events are ordered. Writers queued behind a slow one
        // have already written into their own instance's buffer and exited.
        if let Some(frame) = read_frame(pipe, early, &me).await {
            // A full queue drops the frame, as a full datagram socket does.
            let _ = tx.try_send(frame);
        }
    }
}

/// The writer's user from its process token, `None` when the process has
/// already exited (a helper writes and exits within microseconds).
fn client_process_sid(pipe: &NamedPipeServer) -> Option<Sid> {
    let pid = win_sys::pipe_client_pid(pipe.as_raw_handle()).ok()?;
    win_sys::process_user_sid(pid).ok()
}

async fn read_frame(
    mut pipe: NamedPipeServer,
    same_user: Option<bool>,
    me: &Sid,
) -> Option<Vec<u8>> {
    let mut data = Vec::new();
    let mut limited = (&mut pipe).take(MAX_FRAME as u64 + 1);
    tokio::time::timeout(READ_TIMEOUT, limited.read_to_end(&mut data))
        .await
        .ok()?
        .ok()?;
    if data.is_empty() || data.len() > MAX_FRAME {
        return None;
    }
    // Impersonation needs a completed read, which we now have.
    if same_user.is_none() && win_sys::pipe_client_token_sid(pipe.as_raw_handle()).ok()? != *me {
        return None;
    }
    Some(data)
}

/// Write one frame to the signal pipe `name` and close it: the `pu`
/// `__agent-signal` helper's send. Waits out `ERROR_PIPE_BUSY` with
/// `WaitNamedPipeW` until `deadline`, and refuses a pipe served by another
/// user.
pub fn send_frame(name: &str, frame: &[u8], deadline: Instant) -> io::Result<()> {
    if frame.len() > MAX_FRAME {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let me = win_sys::current_user_sid()?;
    loop {
        match OpenOptions::new().write(true).open(name) {
            Ok(mut pipe) => {
                let pid = win_sys::pipe_server_pid(pipe.as_raw_handle())?;
                if win_sys::process_user_sid(pid)? != me {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        format!("{name} is served by another user"),
                    ));
                }
                return pipe.write_all(frame);
            }
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY) => {
                let left = deadline.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    return Err(io::ErrorKind::TimedOut.into());
                }
                win_sys::wait_named_pipe(name, left);
            }
            Err(e) => return Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name() -> String {
        let home = tempfile::tempdir().unwrap();
        let nonce = format!("test-{}-{:?}", std::process::id(), Instant::now());
        pipe_name(
            &home.path().join("daemon.sock"),
            &nonce.replace(['.', ' '], ""),
        )
        .unwrap()
    }

    #[test]
    fn given_socket_and_nonce_should_name_a_signal_pipe() {
        let home = tempfile::tempdir().unwrap();
        let socket = home.path().join("daemon.sock");
        let a = pipe_name(&socket, "a").unwrap();
        assert!(
            a.starts_with(r"\\.\pipe\purepoint-") && a.ends_with("-signal-a"),
            "{a}"
        );
        let daemon = naming::pipe_name_for_socket(&socket).unwrap();
        assert_eq!(
            a[..a.len() - "signal-a".len()],
            daemon[..daemon.len() - "daemon".len()]
        );
        assert_ne!(a, pipe_name(&socket, "b").unwrap());
    }

    // Writers block their threads; the accept loop needs a free worker.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn given_frames_from_many_writers_should_receive_each_once() {
        let name = name();
        let listener = SignalListener::bind(&name).unwrap();
        assert!(
            SignalListener::bind(&name).is_err(),
            "first instance is exclusive"
        );
        let writers: Vec<_> = (0..32)
            .map(|i| {
                let name = name.clone();
                std::thread::spawn(move || {
                    let deadline = Instant::now() + Duration::from_secs(5);
                    send_frame(&name, format!("{{\"n\":{i}}}").as_bytes(), deadline).unwrap();
                })
            })
            .collect();
        for w in writers {
            w.join().unwrap();
        }
        let mut seen = std::collections::BTreeSet::new();
        let mut buf = vec![0; MAX_FRAME];
        for _ in 0..32 {
            let n = tokio::time::timeout(Duration::from_secs(5), listener.recv(&mut buf))
                .await
                .unwrap()
                .unwrap();
            seen.insert(String::from_utf8(buf[..n].to_vec()).unwrap());
        }
        assert_eq!(seen.len(), 32);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn given_writer_already_closed_should_identify_it_by_impersonation() {
        let name = name();
        let security = PipeSecurity::current_user().unwrap();
        let server = security.create(&name, true).unwrap();
        let n2 = name.clone();
        let deadline = Instant::now() + Duration::from_secs(5);
        std::thread::spawn(move || send_frame(&n2, b"{}", deadline).unwrap())
            .join()
            .unwrap();
        server.connect().await.unwrap();
        let me = win_sys::current_user_sid().unwrap();
        assert_eq!(read_frame(server, None, &me).await.unwrap(), b"{}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn given_oversized_or_silent_writer_should_drop_it_and_keep_serving() {
        let name = name();
        let listener = SignalListener::bind(&name).unwrap();
        assert!(send_frame(&name, &vec![b'x'; MAX_FRAME + 1], Instant::now()).is_err());
        // A writer that connects and never closes is cut off after 1 s.
        let stalled = OpenOptions::new().write(true).open(&name).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let n2 = name.clone();
        std::thread::spawn(move || send_frame(&n2, b"after", deadline).unwrap())
            .join()
            .unwrap();
        let mut buf = vec![0; MAX_FRAME];
        let n = tokio::time::timeout(Duration::from_secs(5), listener.recv(&mut buf))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&buf[..n], b"after");
        drop(stalled);
        drop(listener);
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(send_frame(&name, b"gone", Instant::now()).is_err());
    }
}

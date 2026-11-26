//! NDJSON server on named pipes: private DACL, first-instance guard, client
//! token check, connection cap with a BUSY line, PeekNamedPipe disconnects.
use std::io;
use std::os::windows::io::AsRawHandle;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering::SeqCst};
use std::time::{Duration, Instant};

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::windows::named_pipe::NamedPipeServer;
use tokio::sync::{Notify, Semaphore};

use crate::win::{self, PeerState, PipeSecurity, Sid};
use crate::{FIRST_LINE_TIMEOUT, MAX_CONNECTIONS, MAX_MESSAGE_SIZE};

const MAX_BUSY_WRITERS: usize = 128;
const BUSY_WRITE_TIMEOUT: Duration = Duration::from_secs(1);

pub struct Config {
    pub name: String,
    pub max_connections: usize,
    pub first_line_timeout: Duration,
    /// SID a client's primary token must carry. `None` = the server's own.
    /// Tests inject a different SID to drive the rejection path.
    pub expected_client_sid: Option<Sid>,
}

impl Config {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            max_connections: MAX_CONNECTIONS,
            first_line_timeout: FIRST_LINE_TIMEOUT,
            expected_client_sid: None,
        }
    }
}

#[derive(Default, Debug)]
pub struct Stats {
    pub accepted: AtomicUsize,
    pub busy: AtomicUsize,
    pub busy_write_failed: AtomicUsize,
    pub rejected_sid: AtomicUsize,
    pub disconnects_detected: AtomicUsize,
    pub last_client_pid: AtomicU32,
}

pub struct Server {
    cfg: Config,
    security: PipeSecurity,
    expected: Sid,
    listening: NamedPipeServer,
    busy_writers: Arc<Semaphore>,
    pub stats: Arc<Stats>,
}

impl Server {
    /// Creates the first instance with FILE_FLAG_FIRST_PIPE_INSTANCE; fails
    /// with ERROR_ACCESS_DENIED (5) if the name is already taken (squatter
    /// or a second daemon).
    pub fn bind(cfg: Config) -> io::Result<Self> {
        let security = PipeSecurity::current_user()?;
        let expected = match &cfg.expected_client_sid {
            Some(s) => s.clone(),
            None => win::current_user_sid()?,
        };
        let listening = security.create(&cfg.name, true)?;
        Ok(Self {
            cfg,
            security,
            expected,
            listening,
            busy_writers: Arc::new(Semaphore::new(MAX_BUSY_WRITERS)),
            stats: Arc::default(),
        })
    }

    pub fn sddl(&self) -> &str {
        &self.security.sddl
    }

    pub async fn run(mut self, shutdown: Arc<Notify>) -> io::Result<()> {
        let limit = Arc::new(Semaphore::new(self.cfg.max_connections));
        loop {
            tokio::select! {
                r = self.listening.connect() => {
                    // Replace before dropping so at least one instance always
                    // exists: with zero instances clients get FILE_NOT_FOUND
                    // instead of a retryable PIPE_BUSY.
                    let next = self.security.create(&self.cfg.name, false)?;
                    let connected = std::mem::replace(&mut self.listening, next);
                    if let Err(e) = r {
                        // e.g. ERROR_NO_DATA: client came and went.
                        eprintln!("connect failed: {e}");
                        continue;
                    }
                    self.on_connected(connected, &limit);
                }
                _ = shutdown.notified() => return Ok(()),
            }
        }
    }

    fn on_connected(&self, pipe: NamedPipeServer, limit: &Arc<Semaphore>) {
        match verify_client(&pipe, &self.expected) {
            Ok(pid) => self.stats.last_client_pid.store(pid, SeqCst),
            Err(e) => {
                self.stats.rejected_sid.fetch_add(1, SeqCst);
                eprintln!("rejecting client: {e}");
                return; // drop closes the instance
            }
        }
        let Ok(permit) = limit.clone().try_acquire_owned() else {
            self.stats.busy.fetch_add(1, SeqCst);
            reject_busy(pipe, self.busy_writers.clone(), self.stats.clone());
            return;
        };
        self.stats.accepted.fetch_add(1, SeqCst);
        let stats = self.stats.clone();
        let timeout = self.cfg.first_line_timeout;
        tokio::spawn(async move {
            let _permit = permit;
            handle_connection(pipe, stats, timeout).await;
        });
    }
}

/// GetNamedPipeClientProcessId -> OpenProcess -> token user SID == expected.
pub fn verify_client(pipe: &NamedPipeServer, expected: &Sid) -> io::Result<u32> {
    let pid = win::pipe_client_pid(pipe.as_raw_handle())?;
    let sid = win::process_user_sid(pid)?;
    if &sid != expected {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("client pid {pid} runs as {sid}, expected {expected}"),
        ));
    }
    Ok(pid)
}

/// BUSY reply for a connection over the cap. Unlike a Unix socket, a
/// freshly connected tokio pipe has no write readiness yet, so `try_write`
/// returns WouldBlock for some instances (9/76 in one run). The reply is an
/// awaited write instead, bounded in time and by its own small semaphore so
/// a flood of over-cap clients cannot pile up tasks; excess ones are just
/// closed.
fn reject_busy(mut pipe: NamedPipeServer, writers: Arc<Semaphore>, stats: Arc<Stats>) {
    let Ok(permit) = writers.try_acquire_owned() else {
        stats.busy_write_failed.fetch_add(1, SeqCst);
        return;
    };
    tokio::spawn(async move {
        let _permit = permit;
        let line = b"{\"type\":\"error\",\"code\":\"BUSY\",\"message\":\"daemon connection limit reached\"}
";
        let write = tokio::time::timeout(BUSY_WRITE_TIMEOUT, pipe.write_all(line));
        if !matches!(write.await, Ok(Ok(()))) {
            stats.busy_write_failed.fetch_add(1, SeqCst);
        }
    });
}

async fn handle_connection(pipe: NamedPipeServer, stats: Arc<Stats>, first_timeout: Duration) {
    let raw = pipe.as_raw_handle() as usize;
    let (reader, mut writer) = tokio::io::split(pipe);
    let mut reader = BufReader::new(reader);
    let mut first = true;
    loop {
        let mut line = Vec::new();
        let mut limited = (&mut reader).take(MAX_MESSAGE_SIZE);
        let read = limited.read_until(b'\n', &mut line);
        let n = if first {
            match tokio::time::timeout(first_timeout, read).await {
                Ok(r) => r,
                Err(_) => return,
            }
        } else {
            read.await
        };
        first = false;
        match n {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        if !line.ends_with(b"\n") && line.len() as u64 >= MAX_MESSAGE_SIZE {
            let _ = writer
                .write_all(b"{\"type\":\"error\",\"code\":\"FRAME_TOO_LARGE\"}\n")
                .await;
            return;
        }
        let request: serde_json::Value = match serde_json::from_slice(&line) {
            Ok(v) => v,
            Err(_) => serde_json::json!({}),
        };
        let response = match request["type"].as_str() {
            Some("health") => serde_json::json!({
                "type": "health_report",
                "pid": std::process::id(),
                "transport": "named_pipe",
            }),
            Some("slow") => {
                let ms = request["ms"].as_u64().unwrap_or(1000);
                if operation_until(raw, Duration::from_millis(ms)).await {
                    stats.disconnects_detected.fetch_add(1, SeqCst);
                    return;
                }
                serde_json::json!({"type": "ok"})
            }
            Some("peek") => {
                let ms = request["ms"].as_u64().unwrap_or(0);
                tokio::time::sleep(Duration::from_millis(ms)).await;
                let state = win::peek(raw as _);
                serde_json::json!({"type": "peek", "state": format!("{state:?}")})
            }
            _ => serde_json::json!({"type": "error", "code": "unknown_request"}),
        };
        let mut out = serde_json::to_vec(&response).unwrap();
        out.push(b'\n');
        if writer.write_all(&out).await.is_err() {
            return;
        }
    }
}

/// Simulates a long operation, polling `operation_disconnected` like the
/// daemon does. Returns true if the client vanished first.
async fn operation_until(raw: usize, total: Duration) -> bool {
    let end = Instant::now() + total;
    while Instant::now() < end {
        tokio::time::sleep(Duration::from_millis(10)).await;
        if operation_disconnected(raw as _) {
            return true;
        }
    }
    false
}

/// Windows counterpart of the daemon's recv(MSG_PEEK): a zero-byte
/// PeekNamedPipe fails with ERROR_BROKEN_PIPE once the client closed, and
/// never consumes payload.
pub fn operation_disconnected(raw: std::os::windows::io::RawHandle) -> bool {
    matches!(win::peek(raw), PeerState::Gone(_))
}

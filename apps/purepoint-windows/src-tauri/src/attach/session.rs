//! Port of `Services/DaemonAttachSession.swift`: one attach connection per terminal pane.
//!
//! A session owns one dedicated daemon connection (REQ-WINAPP-059). It sends
//! `attach{agent_id, client_id: "win:<uuid>", replay: "snapshot"}` (the Mac sends `snapshot`
//! whenever it identifies itself), then forwards what the daemon streams to an
//! [`AttachSink`]: PTY output as raw bytes, everything else as an [`AttachEvent`]. Input,
//! `resize` and `claim_size` go out on the same connection.
//!
//! Retries follow the Swift loop exactly:
//! - the daemon closing an attached stream means the agent exited ([`AttachEvent::Exited`]);
//!   no reattach, since every attach replays the whole buffer again;
//! - `AGENT_NOT_FOUND` means the agent is gone ([`AttachEvent::Gone`]);
//! - any other failure reconnects with [`Backoff::SWIFT`] (5 tries 100 ms apart, then 2 s,
//!   20 in all; like the Mac, the count is not reset by a successful attach).
//!
//! The ESC c rule: the daemon replays its buffer on every attach, so a terminal that already
//! shows output gets [`AttachEvent::Reset`] right before the first replayed chunk. That holds
//! for the first attach when the pane asks for it (`reset_before_replay`) and for every
//! reattach within the session. `resync` is forwarded for the view to reset as well.
//!
//! SwiftTerm output filters (`filterTerminalOutput`, `TerminalOutputFilterTests`) are not
//! ported here. They strip DEC 2026 synchronized-output brackets (SwiftTerm#203) and clamp
//! `CSI n A` to the viewport (SwiftTerm overshooting row 0). xterm.js implements mode 2026
//! and clamps cursor-up at the top margin itself, so the bytes reach it unchanged; WIN-7.3,
//! which owns the terminal view, ports a filter only if xterm.js turns out to need one.

use std::sync::Arc;
use std::time::Duration;

use pu_core::protocol::{Replay, Request, Response};
use serde::Serialize;
use tokio::sync::{mpsc, oneshot};

use crate::daemon::client::{BoxWrite, DaemonClient, DaemonError, parse, write_request};

/// What the session tells the view, besides output bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum AttachEvent {
    /// The daemon accepted the attach and will replay `buffered_bytes` before live output.
    AttachReady { buffered_bytes: usize },
    /// Reset the terminal (feed ESC c) before the replay that follows.
    Reset,
    /// The replay is complete; output from here on is live.
    Live,
    /// The daemon could not continue the stream seamlessly: reset, then render what follows.
    Resync,
    /// The PTY size the daemon applied; `owned` when this session's client owns it.
    PtySize {
        cols: u16,
        rows: u16,
        owner_client_id: Option<String>,
        owned: bool,
    },
    /// The connection failed and the session reconnects after `delay_ms`.
    Retrying {
        attempt: u32,
        delay_ms: u64,
        message: String,
    },
    /// The daemon ended the stream: the agent exited. The session does not reattach.
    Exited,
    /// The daemon no longer knows the agent.
    Gone,
    /// The retries ran out.
    Failed { message: String },
}

/// Where a session delivers what the daemon streams.
pub trait AttachSink: Send + Sync + 'static {
    fn event(&self, event: AttachEvent);
    /// Decoded PTY bytes, in stream order with the events.
    fn output(&self, bytes: Vec<u8>);
}

/// The reconnect schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Backoff {
    pub fast_retries: u32,
    pub fast_delay: Duration,
    pub slow_delay: Duration,
    pub max_retries: u32,
}

impl Backoff {
    /// `DaemonAttachSession.start()`.
    pub const SWIFT: Self = Self {
        fast_retries: 5,
        fast_delay: Duration::from_millis(100),
        slow_delay: Duration::from_secs(2),
        max_retries: 20,
    };

    /// The wait before retry number `retry` (1-based), or `None` once retries are spent.
    pub fn delay(&self, retry: u32) -> Option<Duration> {
        if retry == 0 || retry > self.max_retries {
            None
        } else if retry <= self.fast_retries {
            Some(self.fast_delay)
        } else {
            Some(self.slow_delay)
        }
    }
}

/// How a pane opens its session.
#[derive(Debug, Clone)]
pub struct AttachOptions {
    pub agent_id: String,
    /// The pane's size, claimed when it has focus.
    pub cols: u16,
    pub rows: u16,
    pub focused: bool,
    /// The terminal already shows output (a pane re-created over an old view).
    pub reset_before_replay: bool,
    pub backoff: Backoff,
}

impl AttachOptions {
    pub fn new(agent_id: impl Into<String>, cols: u16, rows: u16) -> Self {
        Self {
            agent_id: agent_id.into(),
            cols,
            rows,
            focused: false,
            reset_before_replay: false,
            backoff: Backoff::SWIFT,
        }
    }
}

enum Command {
    Input {
        data: Vec<u8>,
        written: Option<oneshot::Sender<bool>>,
    },
    Resize {
        cols: u16,
        rows: u16,
    },
    ClaimSize,
    Blur,
}

/// Aborts the session task when the session is dropped.
struct TaskGuard(tokio::task::JoinHandle<()>);

impl Drop for TaskGuard {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// A running attach session. Dropping it closes the connection.
pub struct AttachSession {
    agent_id: String,
    client_id: String,
    commands: mpsc::UnboundedSender<Command>,
    _task: TaskGuard,
}

impl AttachSession {
    /// Start streaming on the current Tokio runtime.
    pub fn start(client: DaemonClient, options: AttachOptions, sink: Arc<dyn AttachSink>) -> Self {
        let client_id = format!("win:{}", uuid::Uuid::new_v4().as_hyphenated());
        let (commands, rx) = mpsc::unbounded_channel();
        let worker = Worker {
            client,
            agent_id: options.agent_id.clone(),
            client_id: client_id.clone(),
            desired: (options.cols, options.rows),
            focused: options.focused,
            reset_before_replay: options.reset_before_replay,
            backoff: options.backoff,
            sink,
            commands: rx,
        };
        Self {
            agent_id: options.agent_id,
            client_id,
            commands,
            _task: TaskGuard(tokio::spawn(worker.run())),
        }
    }

    pub fn agent_id(&self) -> &str {
        &self.agent_id
    }

    /// The `client_id` this session attaches with.
    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    /// Terminal input (`sendInput`): dropped while the connection is down.
    pub fn input(&self, data: Vec<u8>) {
        let _ = self.commands.send(Command::Input {
            data,
            written: None,
        });
    }

    /// Input that must not be lost (`deliverInput`): waits up to `timeout` for a connection
    /// and reports whether the bytes were written.
    pub async fn deliver(&self, data: Vec<u8>, timeout: Duration) -> bool {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let (tx, rx) = oneshot::channel();
            let sent = self.commands.send(Command::Input {
                data: data.clone(),
                written: Some(tx),
            });
            if sent.is_err() {
                return false;
            }
            match rx.await {
                Ok(true) => return true,
                // The session ended.
                Err(_) => return false,
                Ok(false) => {}
            }
            if tokio::time::Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    /// The pane's new size (`sendResize`); ignored when either side is zero.
    pub fn resize(&self, cols: u16, rows: u16) {
        if cols > 0 && rows > 0 {
            let _ = self.commands.send(Command::Resize { cols, rows });
        }
    }

    /// The pane gained focus: claim the PTY size (`claimSize`). Typing never claims.
    pub fn claim_size(&self) {
        let _ = self.commands.send(Command::ClaimSize);
    }

    /// The pane lost focus; a reattach no longer claims the size.
    pub fn blur(&self) {
        let _ = self.commands.send(Command::Blur);
    }
}

/// Why one attach attempt ended.
enum End {
    Closed,
    StreamEnded,
    AgentGone,
    Failed(String),
}

struct Worker {
    client: DaemonClient,
    agent_id: String,
    client_id: String,
    desired: (u16, u16),
    focused: bool,
    reset_before_replay: bool,
    backoff: Backoff,
    sink: Arc<dyn AttachSink>,
    commands: mpsc::UnboundedReceiver<Command>,
}

impl Worker {
    async fn run(mut self) {
        let mut retries = 0;
        loop {
            let message = match self.attach_once().await {
                End::Closed => return,
                End::StreamEnded => return self.sink.event(AttachEvent::Exited),
                End::AgentGone => return self.sink.event(AttachEvent::Gone),
                End::Failed(message) => message,
            };
            retries += 1;
            let Some(delay) = self.backoff.delay(retries) else {
                return self.sink.event(AttachEvent::Failed { message });
            };
            self.sink.event(AttachEvent::Retrying {
                attempt: retries,
                delay_ms: delay.as_millis() as u64,
                message,
            });
            if !self.wait(delay).await {
                return;
            }
        }
    }

    /// Sleep between attempts, still taking commands. False once the session is closed.
    async fn wait(&mut self, delay: Duration) -> bool {
        let sleep = tokio::time::sleep(delay);
        tokio::pin!(sleep);
        loop {
            tokio::select! {
                _ = &mut sleep => return true,
                command = self.commands.recv() => match command {
                    None => return false,
                    Some(command) => self.apply(command, None).await,
                },
            }
        }
    }

    /// Carry out one command; `writer` is the live connection, if any.
    async fn apply(&mut self, command: Command, writer: Option<&mut BoxWrite>) {
        let request = match command {
            Command::Input { data, written } => {
                let ok = match writer {
                    Some(writer) => write_request(
                        writer,
                        &Request::Input {
                            agent_id: self.agent_id.clone(),
                            data,
                            submit: false,
                            cancel_on_disconnect: false,
                        },
                    )
                    .await
                    .is_ok(),
                    None => false,
                };
                if let Some(written) = written {
                    let _ = written.send(ok);
                }
                return;
            }
            Command::Resize { cols, rows } => {
                self.desired = (cols, rows);
                Request::Resize {
                    agent_id: self.agent_id.clone(),
                    cols,
                    rows,
                }
            }
            Command::ClaimSize => {
                self.focused = true;
                self.claim()
            }
            Command::Blur => {
                self.focused = false;
                return;
            }
        };
        if let Some(writer) = writer {
            let _ = write_request(writer, &request).await;
        }
    }

    fn claim(&self) -> Request {
        Request::ClaimSize {
            cols: self.desired.0,
            rows: self.desired.1,
        }
    }

    /// `runAttachLoop`: one connection, from `attach` to its end.
    async fn attach_once(&mut self) -> End {
        // Dropping the session aborts this task, so a connect needs no close check.
        let (mut reader, mut writer) = match self.client.connect().await {
            Ok(halves) => halves,
            Err(error) => return End::Failed(error.to_string()),
        };
        let attach = Request::Attach {
            agent_id: self.agent_id.clone(),
            replay: Some(Replay::Snapshot),
            client_id: Some(self.client_id.clone()),
        };
        if let Err(error) = write_request(&mut writer, &attach).await {
            return End::Failed(error.to_string());
        }

        // A dedicated reader, so a line half read is never lost to `select!`.
        let (lines_tx, mut lines) = mpsc::channel::<Result<Response, DaemonError>>(64);
        let _reader = TaskGuard(tokio::spawn(async move {
            loop {
                let response = match reader.next_line().await {
                    Ok(Some(line)) => fast_output(line).map_or_else(|| parse(line), Ok),
                    Ok(None) => Err(DaemonError::Eof),
                    Err(e) => Err(DaemonError::Io(format!(
                        "Reading from the daemon failed: {e}"
                    ))),
                };
                let last = response.is_err();
                if lines_tx.send(response).await.is_err() || last {
                    return;
                }
            }
        }));

        let mut ready = false;
        let mut replay_remaining: usize = 0;
        let mut first_chunk = true;
        loop {
            tokio::select! {
                biased;
                command = self.commands.recv() => match command {
                    None => return End::Closed,
                    Some(command) => self.apply(command, Some(&mut writer)).await,
                },
                line = lines.recv() => {
                    let response = match line {
                        Some(Ok(response)) => response,
                        // The daemon closes an attached stream when the agent exits.
                        Some(Err(DaemonError::Eof)) | None if ready => return End::StreamEnded,
                        Some(Err(error)) => return End::Failed(error.to_string()),
                        None => return End::Failed(DaemonError::Eof.to_string()),
                    };
                    if let Response::Error { code, message } = &response {
                        return if code == "AGENT_NOT_FOUND" {
                            End::AgentGone
                        } else {
                            End::Failed(format!("Attach failed: {message}"))
                        };
                    }
                    if !ready {
                        let Response::AttachReady { buffered_bytes } = response else {
                            return End::Failed("Unexpected response during attach".into());
                        };
                        ready = true;
                        replay_remaining = buffered_bytes;
                        self.sink.event(AttachEvent::AttachReady { buffered_bytes });
                        if buffered_bytes == 0 {
                            self.sink.event(AttachEvent::Live);
                        }
                        if self.focused {
                            let _ = write_request(&mut writer, &self.claim()).await;
                        }
                        continue;
                    }
                    match response {
                        Response::Resync => {
                            let was_replay = replay_remaining > 0;
                            replay_remaining = 0;
                            self.sink.event(AttachEvent::Resync);
                            if was_replay {
                                self.sink.event(AttachEvent::Live);
                            }
                        }
                        Response::PtySize { cols, rows, owner_client_id } => {
                            let owned = owner_client_id.as_deref() == Some(self.client_id.as_str());
                            self.sink.event(AttachEvent::PtySize { cols, rows, owner_client_id, owned });
                        }
                        Response::Output { data, .. } if !data.is_empty() => {
                            if first_chunk {
                                first_chunk = false;
                                if self.reset_before_replay {
                                    self.sink.event(AttachEvent::Reset);
                                }
                                // A retry within this session replays from the start again.
                                self.reset_before_replay = true;
                            }
                            let was_replay = replay_remaining > 0;
                            replay_remaining = replay_remaining.saturating_sub(data.len());
                            self.sink.output(data);
                            if was_replay && replay_remaining == 0 {
                                self.sink.event(AttachEvent::Live);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

/// An `output` line in the daemon's own field order, hex-decoded with a table; `None`
/// sends the line through the general parser. Output is nearly every byte an attach carries,
/// and serde's per-pair `from_str_radix` was the stream's bottleneck.
pub fn fast_output(line: &[u8]) -> Option<Response> {
    let rest = line.strip_prefix(br#"{"type":"output","agent_id":""#)?;
    let id_end = rest.iter().position(|&b| b == b'"' || b == b'\\')?;
    let (agent_id, rest) = rest.split_at(id_end);
    let hex = rest
        .strip_prefix(br#"","data":""#)?
        .strip_suffix(br#""}"#)?;
    if hex.len() % 2 != 0 {
        return None;
    }
    fn nibble(b: u8) -> Option<u8> {
        match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            b'A'..=b'F' => Some(b - b'A' + 10),
            _ => None,
        }
    }
    let mut data = Vec::with_capacity(hex.len() / 2);
    for pair in hex.chunks_exact(2) {
        data.push(nibble(pair[0])? << 4 | nibble(pair[1])?);
    }
    Some(Response::Output {
        agent_id: String::from_utf8(agent_id.to_vec()).ok()?,
        data,
    })
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;

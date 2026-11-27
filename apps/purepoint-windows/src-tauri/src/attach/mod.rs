//! Terminal attach sessions streaming agent PTYs to xterm.js (WIN-5.4).
//!
//! - `session`: the port of `DaemonAttachSession.swift`, one daemon connection per pane.
//!
//! Tauri surface (front end: `ui/src/services/attach.ts`): `attach_open` returns a session
//! id and streams over the caller's `Channel`: PTY output as raw bytes
//! (`InvokeResponseBody::Raw`, an `ArrayBuffer` in the page, never a JSON number array) and
//! every other message as a JSON [`session::AttachEvent`], in stream order. `attach_input`,
//! `attach_resize`, `attach_claim_size` (pane focus), `attach_blur` and `attach_close` act on
//! that session's connection.

pub mod session;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use serde::Deserialize;
use tauri::State;
use tauri::ipc::{Channel, InvokeResponseBody};

use crate::daemon::DaemonState;
use crate::daemon::client::DaemonError;
use crate::daemon::machine::MachineClients;
use session::{AttachEvent, AttachOptions, AttachSession, AttachSink};

/// How long `attach_input` with `deliver` waits for a connection (`deliverInput`).
pub const DELIVER_TIMEOUT: Duration = Duration::from_secs(5);

/// A Tauri channel as an [`AttachSink`]. The first send that fails (the page that opened the
/// session reloaded or its renderer crashed, so nothing reads the channel) tears the session
/// down: it leaves [`AttachState`], which drops it and closes its daemon connection.
pub struct ChannelSink {
    channel: Channel<InvokeResponseBody>,
    teardown: Teardown,
}

/// Removes one session from the table, once.
struct Teardown {
    failed: AtomicBool,
    id: u32,
    sessions: Weak<Mutex<Sessions>>,
}

impl Teardown {
    fn sent(&self, result: tauri::Result<()>) {
        if result.is_err() && !self.failed.swap(true, Ordering::AcqRel) {
            let removed = self
                .sessions
                .upgrade()
                .and_then(|sessions| lock(&sessions).remove(&self.id));
            // Dropped outside the lock: this aborts the session's own task.
            drop(removed);
        }
    }
}

impl ChannelSink {
    fn new(channel: Channel<InvokeResponseBody>, id: u32, sessions: &Arc<Mutex<Sessions>>) -> Self {
        Self {
            channel,
            teardown: Teardown {
                failed: AtomicBool::new(false),
                id,
                sessions: Arc::downgrade(sessions),
            },
        }
    }

    fn failed(&self) -> bool {
        self.teardown.failed.load(Ordering::Acquire)
    }
}

impl AttachSink for ChannelSink {
    fn event(&self, event: AttachEvent) {
        if self.failed() {
            return;
        }
        if let Ok(json) = serde_json::to_string(&event) {
            self.teardown
                .sent(self.channel.send(InvokeResponseBody::Json(json)));
        }
    }

    fn output(&self, bytes: Vec<u8>) {
        if self.failed() {
            return;
        }
        self.teardown
            .sent(self.channel.send(InvokeResponseBody::Raw(bytes)));
    }
}

type Sessions = HashMap<u32, Arc<AttachSession>>;

fn lock(sessions: &Mutex<Sessions>) -> std::sync::MutexGuard<'_, Sessions> {
    sessions.lock().unwrap_or_else(|e| e.into_inner())
}

/// Tauri state: the open sessions by id.
#[derive(Default)]
pub struct AttachState {
    next_id: AtomicU32,
    sessions: Arc<Mutex<Sessions>>,
}

impl AttachState {
    fn next_id(&self) -> u32 {
        self.next_id.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// Starts a session streaming to `channel` and returns its id.
    pub fn open(
        &self,
        client: crate::daemon::client::DaemonClient,
        options: AttachOptions,
        channel: Channel<InvokeResponseBody>,
    ) -> u32 {
        let id = self.next_id();
        let sink = Arc::new(ChannelSink::new(channel, id, &self.sessions));
        let session = AttachSession::start(client, options, sink.clone());
        let mut sessions = self.lock();
        // The channel may already have failed (a reload raced the open): then never insert.
        if !sink.failed() {
            sessions.insert(id, Arc::new(session));
        }
        id
    }

    fn get(&self, id: u32) -> Result<Arc<AttachSession>, DaemonError> {
        self.lock()
            .get(&id)
            .cloned()
            .ok_or_else(|| DaemonError::InvalidRequest(format!("no attach session {id}")))
    }

    fn remove(&self, id: u32) -> Option<Arc<AttachSession>> {
        self.lock().remove(&id)
    }

    /// Closes every session (the page that opened them is gone). Returns how many.
    pub fn clear(&self) -> usize {
        let drained: Vec<_> = self.lock().drain().collect();
        drained.len()
    }

    pub fn len(&self) -> usize {
        self.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Sessions> {
        lock(&self.sessions)
    }
}

/// Input from the page: text, or bytes as a number array (keystrokes are small).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum InputData {
    Text(String),
    Bytes(Vec<u8>),
}

impl InputData {
    /// The bytes to write. The attach stream writes input verbatim (the daemon ignores
    /// `submit` there), so a submit appends the Enter itself.
    pub fn into_bytes(self, submit: bool) -> Vec<u8> {
        let mut bytes = match self {
            Self::Text(text) => text.into_bytes(),
            Self::Bytes(bytes) => bytes,
        };
        if submit {
            bytes.push(b'\r');
        }
        bytes
    }
}

/// Open a session for `agent_id` (on `machine_id`'s daemon when given) and stream it to
/// `on_event`. Returns the session id.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn attach_open(
    daemon: State<'_, DaemonState>,
    machines: State<'_, MachineClients>,
    attach: State<'_, AttachState>,
    agent_id: String,
    cols: u16,
    rows: u16,
    on_event: Channel<InvokeResponseBody>,
    focused: Option<bool>,
    reset_before_replay: Option<bool>,
    machine_id: Option<String>,
) -> Result<u32, DaemonError> {
    let client = match machine_id {
        Some(machine) => machines.client(&machine),
        None => daemon.lifecycle()?.client().clone(),
    };
    let mut options = AttachOptions::new(agent_id, cols, rows);
    options.focused = focused.unwrap_or(false);
    options.reset_before_replay = reset_before_replay.unwrap_or(false);
    Ok(attach.open(client, options, on_event))
}

/// Send input on the session's connection. With `deliver`, waits up to 5 s for a
/// connection and returns whether the bytes were written; otherwise returns at once.
#[tauri::command]
pub async fn attach_input(
    attach: State<'_, AttachState>,
    session_id: u32,
    data: InputData,
    submit: Option<bool>,
    deliver: Option<bool>,
) -> Result<bool, DaemonError> {
    let session = attach.get(session_id)?;
    let bytes = data.into_bytes(submit.unwrap_or(false));
    if deliver.unwrap_or(false) {
        Ok(session.deliver(bytes, DELIVER_TIMEOUT).await)
    } else {
        session.input(bytes);
        Ok(true)
    }
}

/// The pane's new size in cells.
#[tauri::command]
pub fn attach_resize(
    attach: State<'_, AttachState>,
    session_id: u32,
    cols: u16,
    rows: u16,
) -> Result<(), DaemonError> {
    attach.get(session_id)?.resize(cols, rows);
    Ok(())
}

/// The pane gained focus: claim the PTY size. Call only on focus.
#[tauri::command]
pub fn attach_claim_size(
    attach: State<'_, AttachState>,
    session_id: u32,
) -> Result<(), DaemonError> {
    attach.get(session_id)?.claim_size();
    Ok(())
}

/// The pane lost focus.
#[tauri::command]
pub fn attach_blur(attach: State<'_, AttachState>, session_id: u32) -> Result<(), DaemonError> {
    attach.get(session_id)?.blur();
    Ok(())
}

/// Close the session and its connection. Closing an unknown id is a no-op.
#[tauri::command]
pub fn attach_close(attach: State<'_, AttachState>, session_id: u32) {
    attach.remove(session_id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_input_forms_should_decode_text_and_bytes() {
        let text: InputData = serde_json::from_str(r#""ls""#).unwrap();
        assert_eq!(text.into_bytes(true), b"ls\r");
        let bytes: InputData = serde_json::from_str("[27, 91, 65]").unwrap();
        assert_eq!(bytes.into_bytes(false), b"\x1b[A");
    }

    #[test]
    fn given_events_should_serialize_tagged_camel_case() {
        let value = serde_json::to_value(AttachEvent::PtySize {
            cols: 80,
            rows: 24,
            owner_client_id: Some("win:x".into()),
            owned: true,
        })
        .unwrap();
        assert_eq!(
            value,
            serde_json::json!({"type": "pty_size", "cols": 80, "rows": 24, "ownerClientId": "win:x", "owned": true})
        );
        let ready = serde_json::to_value(AttachEvent::AttachReady { buffered_bytes: 3 }).unwrap();
        assert_eq!(
            ready,
            serde_json::json!({"type": "attach_ready", "bufferedBytes": 3})
        );
        assert_eq!(
            serde_json::to_value(AttachEvent::Exited).unwrap(),
            serde_json::json!({"type": "exited"})
        );
    }

    #[test]
    fn given_channel_sink_should_send_output_raw_and_events_as_json() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let record = seen.clone();
        let sessions = Arc::new(Mutex::new(Sessions::new()));
        let sink = ChannelSink::new(
            Channel::new(move |body| {
                record.lock().unwrap().push(body);
                Ok(())
            }),
            1,
            &sessions,
        );
        sink.output(vec![0, 1, 255]);
        sink.event(AttachEvent::Live);
        let seen = seen.lock().unwrap();
        assert!(matches!(&seen[0], InvokeResponseBody::Raw(bytes) if bytes == &[0, 1, 255]));
        assert!(matches!(&seen[1], InvokeResponseBody::Json(json) if json == r#"{"type":"live"}"#));
        assert!(!sink.failed());
    }

    /// A client whose daemon is not running: the session retries, emitting events.
    fn absent_daemon() -> (tempfile::TempDir, crate::daemon::client::DaemonClient) {
        let dir = tempfile::tempdir().unwrap();
        let client =
            crate::daemon::client::DaemonClient::local(&dir.path().join("pu.sock")).unwrap();
        (dir, client)
    }

    #[tokio::test]
    async fn given_a_failed_channel_send_should_tear_the_session_down() {
        let (_dir, client) = absent_daemon();
        let attach = AttachState::default();
        let sends = Arc::new(AtomicU32::new(0));
        let counted = sends.clone();
        // The page is gone: every send fails.
        let channel = Channel::new(move |_| {
            counted.fetch_add(1, Ordering::Relaxed);
            Err(tauri::Error::WebviewNotFound)
        });
        let id = attach.open(client, AttachOptions::new("ag-1", 80, 24), channel);
        for _ in 0..250 {
            if attach.is_empty() && sends.load(Ordering::Relaxed) > 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(attach.is_empty(), "session {id} is still open");
        assert!(attach.get(id).is_err());
        let after = sends.load(Ordering::Relaxed);
        assert!(after >= 1);
        tokio::time::sleep(Duration::from_millis(300)).await;
        // Nothing more is sent once torn down.
        assert_eq!(sends.load(Ordering::Relaxed), after);
    }

    #[tokio::test]
    async fn given_a_reload_should_clear_every_session() {
        let (_dir, client) = absent_daemon();
        let attach = AttachState::default();
        let quiet = || Channel::new(|_| Ok(()));
        let a = attach.open(client.clone(), AttachOptions::new("ag-1", 80, 24), quiet());
        let b = attach.open(client, AttachOptions::new("ag-2", 80, 24), quiet());
        assert_eq!(attach.len(), 2);
        assert_eq!(attach.clear(), 2);
        assert!(attach.is_empty());
        assert!(attach.get(a).is_err() && attach.get(b).is_err());
        assert_eq!(attach.clear(), 0);
    }
}

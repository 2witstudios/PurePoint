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
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
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

/// A Tauri channel as an [`AttachSink`].
pub struct ChannelSink(pub Channel<InvokeResponseBody>);

impl AttachSink for ChannelSink {
    fn event(&self, event: AttachEvent) {
        if let Ok(json) = serde_json::to_string(&event) {
            let _ = self.0.send(InvokeResponseBody::Json(json));
        }
    }

    fn output(&self, bytes: Vec<u8>) {
        let _ = self.0.send(InvokeResponseBody::Raw(bytes));
    }
}

/// Tauri state: the open sessions by id.
#[derive(Default)]
pub struct AttachState {
    next_id: AtomicU32,
    sessions: Mutex<HashMap<u32, Arc<AttachSession>>>,
}

impl AttachState {
    fn insert(&self, session: AttachSession) -> u32 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        self.lock().insert(id, Arc::new(session));
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

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<u32, Arc<AttachSession>>> {
        self.sessions.lock().unwrap_or_else(|e| e.into_inner())
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
    let session = AttachSession::start(client, options, Arc::new(ChannelSink(on_event)));
    Ok(attach.insert(session))
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
        let sink = ChannelSink(Channel::new(move |body| {
            record.lock().unwrap().push(body);
            Ok(())
        }));
        sink.output(vec![0, 1, 255]);
        sink.event(AttachEvent::Live);
        let seen = seen.lock().unwrap();
        assert!(matches!(&seen[0], InvokeResponseBody::Raw(bytes) if bytes == &[0, 1, 255]));
        assert!(matches!(&seen[1], InvokeResponseBody::Json(json) if json == r#"{"type":"live"}"#));
    }
}

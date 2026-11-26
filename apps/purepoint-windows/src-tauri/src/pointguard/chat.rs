//! The Point Guard chat WebSocket (WIN-9.4): the transport under `ui/src/state/piChat.ts`.
//!
//! The bridge authenticates the v1 chat socket only by the upgrade request's
//! `Authorization: Bearer <secret>` and `X-PointGuard-Client-ID` headers (`network.js`), and
//! refuses any request carrying `Origin`. A page cannot set either header on a browser
//! WebSocket, and WebView2 always sends `Origin`, so the backend holds the socket
//! (`tokio-tungstenite`) and relays its text records to the page over a Tauri `Channel`,
//! exactly as the Mac's `URLSessionWebSocketTask` sends them.
//!
//! Only the local managed bridge is reachable this way: `ws://127.0.0.1:<port>/v1`, the
//! descriptor's `nativeChatURL`. The bridge refuses plaintext authorization anywhere else.
//!
//! Tauri surface: `pointguard_chat_open` connects and returns a socket id, streaming
//! [`ChatSocketEvent`]s over the caller's channel; `pointguard_chat_send` sends one text
//! record; `pointguard_chat_close` closes with 1001 (going away), as the Mac's `cancel(with:
//! .goingAway)` does.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use tauri::State;
use tauri::ipc::Channel;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::protocol::frame::CloseFrame;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::{Error as WsError, Message};

use super::runtime;

/// The Mac's `maximumMessageSize`: snapshots are bounded well below it.
pub const MAX_MESSAGE: usize = 4 * 1024 * 1024;
/// How long the TCP connect and upgrade may take.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// What the page hears from one socket, in order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ChatSocketEvent {
    /// One text record from the bridge (a snapshot, receipt or editor offer).
    Message { text: String },
    /// The socket ended without the page asking: a close frame (`code`, e.g. 1009 or 4001)
    /// or a transport failure (`code` null). Nothing follows.
    Closed { code: Option<u16>, reason: String },
}

/// Where a socket's events go: the page's channel, or a test's recorder.
pub trait ChatSink: Send + Sync + 'static {
    /// False when the event could not be delivered (the page reloaded or its renderer
    /// crashed): the socket then closes and leaves the table.
    fn event(&self, event: ChatSocketEvent) -> bool;
}

impl ChatSink for Channel<ChatSocketEvent> {
    fn event(&self, event: ChatSocketEvent) -> bool {
        self.send(event).is_ok()
    }
}

enum Outgoing {
    Text(String),
    Close,
}

/// The upgrade request's credentials, checked before anything is sent.
pub fn validate(endpoint: &str, secret: &str, client_id: &str) -> Result<(), String> {
    if !runtime::local_url(endpoint, "ws", "/v1") {
        return Err("Point Guard chat connects only to this PC's bridge (ws://127.0.0.1:<port>/v1). Restart Point Guard.".into());
    }
    if secret.chars().count() < 32
        || secret.len() > 1024
        || !secret.bytes().all(|b| (0x20..0x7f).contains(&b))
    {
        return Err("Enter the pairing secret from Point Guard (at least 32 characters).".into());
    }
    if client_id.is_empty()
        || client_id.len() > 100
        || !client_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err("Invalid managed desktop identity.".into());
    }
    Ok(())
}

/// The bridge's upgrade refusals (`network.js`), as the user should read them.
fn refusal(status: u16) -> String {
    match status {
        401 => "Point Guard rejected the pairing secret. Restart Point Guard to refresh it.".into(),
        403 => "Point Guard refused this client identity. Restart Point Guard.".into(),
        503 => "Point Guard has too many chat connections. Close another window and retry.".into(),
        other => format!("Point Guard refused the chat connection ({other})."),
    }
}

fn connect_error(error: WsError) -> String {
    match error {
        WsError::Http(response) => refusal(response.status().as_u16()),
        WsError::Io(e) => format!("Could not reach Point Guard: {e}."),
        other => format!("Could not connect to Point Guard: {other}."),
    }
}

/// Close with 1001 (going away), as the Mac's `cancel(with: .goingAway)` does.
async fn close_away<S>(write: &mut S)
where
    S: futures_util::Sink<Message> + Unpin,
{
    let frame = CloseFrame {
        code: CloseCode::Away,
        reason: "".into(),
    };
    let _ = write.send(Message::Close(Some(frame))).await;
    let _ = write.close().await;
}

/// One open chat socket.
pub struct ChatSocket {
    outgoing: mpsc::UnboundedSender<Outgoing>,
}

impl ChatSocket {
    /// Connect with the Mac's headers, then pump records to `sink` until the socket ends.
    /// `on_end` runs once the pump stops, however it stopped.
    pub async fn connect(
        endpoint: &str,
        secret: &str,
        client_id: &str,
        sink: Arc<dyn ChatSink>,
        on_end: impl FnOnce() + Send + 'static,
    ) -> Result<Self, String> {
        validate(endpoint, secret, client_id)?;
        let mut request = endpoint
            .into_client_request()
            .map_err(|e| format!("Invalid Point Guard address: {e}."))?;
        let headers = request.headers_mut();
        headers.insert(
            "Authorization",
            HeaderValue::from_str(&format!("Bearer {secret}"))
                .map_err(|_| "Invalid pairing secret.".to_string())?,
        );
        headers.insert(
            "X-PointGuard-Client-ID",
            HeaderValue::from_str(client_id).map_err(|_| "Invalid client identity.".to_string())?,
        );
        let config = WebSocketConfig::default()
            .max_message_size(Some(MAX_MESSAGE))
            .max_frame_size(Some(MAX_MESSAGE));
        let (stream, _) = tokio::time::timeout(
            CONNECT_TIMEOUT,
            tokio_tungstenite::connect_async_with_config(request, Some(config), true),
        )
        .await
        .map_err(|_| "Point Guard did not answer the chat connection in time.".to_string())?
        .map_err(connect_error)?;
        let (outgoing, mut queue) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            let (mut write, mut read) = stream.split();
            loop {
                tokio::select! {
                    next = queue.recv() => match next {
                        Some(Outgoing::Text(text)) => {
                            if let Err(e) = write.send(Message::text(text)).await {
                                sink.event(ChatSocketEvent::Closed { code: None, reason: e.to_string() });
                                break;
                            }
                        }
                        Some(Outgoing::Close) | None => {
                            close_away(&mut write).await;
                            break;
                        }
                    },
                    record = read.next() => match record {
                        Some(Ok(Message::Text(text))) => {
                            if !sink.event(ChatSocketEvent::Message { text: text.as_str().to_owned() }) {
                                close_away(&mut write).await;
                                break;
                            }
                        }
                        Some(Ok(Message::Binary(bytes))) => match String::from_utf8(bytes.to_vec()) {
                            Ok(text) => {
                                if !sink.event(ChatSocketEvent::Message { text }) {
                                    close_away(&mut write).await;
                                    break;
                                }
                            }
                            Err(_) => {
                                sink.event(ChatSocketEvent::Closed {
                                    code: None,
                                    reason: "Unsupported message from bridge.".into(),
                                });
                                break;
                            }
                        },
                        Some(Ok(Message::Close(frame))) => {
                            let (code, reason) = frame
                                .map(|f| (Some(u16::from(f.code)), f.reason.as_str().to_owned()))
                                .unwrap_or((None, String::new()));
                            sink.event(ChatSocketEvent::Closed { code, reason });
                            break;
                        }
                        Some(Ok(_)) => {}
                        Some(Err(e)) => {
                            sink.event(ChatSocketEvent::Closed { code: None, reason: e.to_string() });
                            break;
                        }
                        None => {
                            sink.event(ChatSocketEvent::Closed {
                                code: None,
                                reason: "Point Guard closed the connection.".into(),
                            });
                            break;
                        }
                    },
                }
            }
            on_end();
        });
        Ok(Self { outgoing })
    }

    /// Queue one text record. Fails once the socket has ended.
    pub fn send(&self, text: String) -> Result<(), String> {
        self.outgoing
            .send(Outgoing::Text(text))
            .map_err(|_| "Connect to Point Guard before sending.".to_string())
    }

    /// Close with 1001. No `Closed` event follows a close the page asked for.
    pub fn close(&self) {
        let _ = self.outgoing.send(Outgoing::Close);
    }
}

/// Tauri state: the open chat sockets by id.
#[derive(Default, Clone)]
pub struct ChatSockets {
    next_id: Arc<AtomicU32>,
    open: Arc<Mutex<Table>>,
}

#[derive(Default)]
struct Table {
    sockets: HashMap<u32, ChatSocket>,
    /// Sockets whose pump ended before `open` could insert them.
    ended: std::collections::HashSet<u32>,
}

impl ChatSockets {
    fn table(&self) -> std::sync::MutexGuard<'_, Table> {
        self.open.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Open a socket; it leaves the table by itself when it ends.
    pub async fn open(
        &self,
        endpoint: &str,
        secret: &str,
        client_id: &str,
        sink: Arc<dyn ChatSink>,
    ) -> Result<u32, String> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        let table = self.clone();
        let socket = ChatSocket::connect(endpoint, secret, client_id, sink, move || {
            let mut table = table.table();
            if table.sockets.remove(&id).is_none() {
                table.ended.insert(id);
            }
        })
        .await?;
        let mut table = self.table();
        if !table.ended.remove(&id) {
            table.sockets.insert(id, socket);
        }
        Ok(id)
    }

    pub fn send(&self, id: u32, text: String) -> Result<(), String> {
        match self.table().sockets.get(&id) {
            Some(socket) => socket.send(text),
            None => Err("Connect to Point Guard before sending.".into()),
        }
    }

    pub fn close(&self, id: u32) {
        if let Some(socket) = self.table().sockets.remove(&id) {
            socket.close();
        }
    }

    /// Closes every socket (the page that opened them is gone). Returns how many.
    pub fn clear(&self) -> usize {
        let drained: Vec<ChatSocket> = self.table().sockets.drain().map(|(_, s)| s).collect();
        for socket in &drained {
            socket.close();
        }
        drained.len()
    }

    pub fn len(&self) -> usize {
        self.table().sockets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Connect to the local bridge with the desktop chat credentials and stream its records to
/// `on_event`. Returns the socket id.
#[tauri::command]
pub async fn pointguard_chat_open(
    sockets: State<'_, ChatSockets>,
    endpoint: String,
    secret: String,
    client_id: String,
    on_event: Channel<ChatSocketEvent>,
) -> Result<u32, String> {
    sockets
        .open(&endpoint, &secret, &client_id, Arc::new(on_event))
        .await
}

/// Send one text record on the socket.
#[tauri::command]
pub fn pointguard_chat_send(
    sockets: State<'_, ChatSockets>,
    socket_id: u32,
    text: String,
) -> Result<(), String> {
    sockets.send(socket_id, text)
}

/// Close the socket (1001, going away).
#[tauri::command]
pub fn pointguard_chat_close(sockets: State<'_, ChatSockets>, socket_id: u32) {
    sockets.close(socket_id);
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;
    use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};

    const SECRET: &str = "0123456789abcdef0123456789abcdef";

    #[derive(Default)]
    struct Recorder(Mutex<Vec<ChatSocketEvent>>, tokio::sync::Notify);

    impl ChatSink for Recorder {
        fn event(&self, event: ChatSocketEvent) -> bool {
            self.0.lock().unwrap().push(event);
            self.1.notify_one();
            true
        }
    }

    /// The page is gone: nothing is delivered.
    struct Gone(std::sync::atomic::AtomicUsize);

    impl ChatSink for Gone {
        fn event(&self, _: ChatSocketEvent) -> bool {
            self.0.fetch_add(1, Ordering::Relaxed);
            false
        }
    }

    async fn until_empty(sockets: &ChatSockets) {
        for _ in 0..100 {
            if sockets.is_empty() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    impl Recorder {
        async fn wait(&self, count: usize) -> Vec<ChatSocketEvent> {
            loop {
                let notified = self.1.notified();
                {
                    let seen = self.0.lock().unwrap();
                    if seen.len() >= count {
                        return seen.clone();
                    }
                }
                tokio::time::timeout(Duration::from_secs(5), notified)
                    .await
                    .expect("timed out waiting for socket events");
            }
        }
    }

    /// A one-connection bridge stand-in that checks the upgrade as `network.js` does, echoes
    /// each text record back as a receipt, and closes 4001 on "revoke".
    async fn bridge() -> (String, tokio::task::JoinHandle<Option<CloseCode>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!(
            "ws://127.0.0.1:{}/v1",
            listener.local_addr().unwrap().port()
        );
        let task = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let check =
                |request: &Request, response: Response| -> Result<Response, ErrorResponse> {
                    let header = |name: &str| {
                        request
                            .headers()
                            .get(name)
                            .and_then(|v| v.to_str().ok())
                            .map(str::to_owned)
                    };
                    let authorized = header("authorization").as_deref()
                        == Some(&format!("Bearer {SECRET}"))
                        && header("x-pointguard-client-id").as_deref() == Some("desktop-1")
                        && header("origin").is_none()
                        && request.uri().path() == "/v1";
                    if authorized {
                        Ok(response)
                    } else {
                        let mut refused = ErrorResponse::new(None);
                        *refused.status_mut() =
                            tokio_tungstenite::tungstenite::http::StatusCode::UNAUTHORIZED;
                        Err(refused)
                    }
                };
            let Ok(mut ws) = tokio_tungstenite::accept_hdr_async(tcp, check).await else {
                return None;
            };
            ws.send(Message::text(r#"{"type":"editor","id":"e1","text":"hi"}"#))
                .await
                .unwrap();
            while let Some(Ok(message)) = ws.next().await {
                match message {
                    Message::Text(text) if text.as_str() == "revoke" => {
                        let frame = CloseFrame {
                            code: CloseCode::from(4001),
                            reason: "Device trust revoked; re-pair deliberately".into(),
                        };
                        let _ = ws.send(Message::Close(Some(frame))).await;
                        return None;
                    }
                    Message::Text(text) => {
                        let receipt = format!(r#"{{"type":"receipt","echo":{}}}"#, text.as_str());
                        ws.send(Message::text(receipt)).await.unwrap();
                    }
                    Message::Close(frame) => return frame.map(|f| f.code),
                    _ => {}
                }
            }
            None
        });
        (url, task)
    }

    #[test]
    fn given_credentials_should_accept_only_the_local_bridge() {
        assert!(validate("ws://127.0.0.1:8787/v1", SECRET, "desktop-1").is_ok());
        for endpoint in [
            "wss://127.0.0.1:8787/v1",
            "ws://100.100.1.2:8787/v1",
            "ws://localhost:8787/v1",
            "ws://127.0.0.1:8787/v2",
            "ws://127.0.0.1:8787/v1?token=x",
        ] {
            assert!(
                validate(endpoint, SECRET, "desktop-1").is_err(),
                "{endpoint}"
            );
        }
        assert!(validate("ws://127.0.0.1:8787/v1", "short", "desktop-1").is_err());
        assert!(
            validate(
                "ws://127.0.0.1:8787/v1",
                &format!("{SECRET}\n"),
                "desktop-1"
            )
            .is_err()
        );
        assert!(validate("ws://127.0.0.1:8787/v1", SECRET, "bad id").is_err());
    }

    #[test]
    fn given_events_should_serialize_as_the_page_reads_them() {
        assert_eq!(
            serde_json::to_value(ChatSocketEvent::Message { text: "x".into() }).unwrap(),
            serde_json::json!({"type": "message", "text": "x"})
        );
        assert_eq!(
            serde_json::to_value(ChatSocketEvent::Closed {
                code: Some(4001),
                reason: "r".into()
            })
            .unwrap(),
            serde_json::json!({"type": "closed", "code": 4001, "reason": "r"})
        );
    }

    #[tokio::test]
    async fn given_the_mac_headers_should_relay_records_both_ways_and_close_going_away() {
        let (url, server) = bridge().await;
        let sockets = ChatSockets::default();
        let recorder = Arc::new(Recorder::default());
        let id = sockets
            .open(&url, SECRET, "desktop-1", recorder.clone())
            .await
            .unwrap();
        sockets.send(id, r#"{"op":"sync"}"#.into()).unwrap();
        let seen = recorder.wait(2).await;
        assert_eq!(
            seen,
            vec![
                ChatSocketEvent::Message {
                    text: r#"{"type":"editor","id":"e1","text":"hi"}"#.into()
                },
                ChatSocketEvent::Message {
                    text: r#"{"type":"receipt","echo":{"op":"sync"}}"#.into()
                },
            ]
        );
        sockets.close(id);
        assert_eq!(server.await.unwrap(), Some(CloseCode::Away));
        assert!(sockets.send(id, "late".into()).is_err());
    }

    #[tokio::test]
    async fn given_a_wrong_secret_should_report_the_refusal() {
        let (url, _server) = bridge().await;
        let error = ChatSockets::default()
            .open(
                &url,
                &"x".repeat(32),
                "desktop-1",
                Arc::new(Recorder::default()),
            )
            .await
            .unwrap_err();
        assert!(error.contains("rejected the pairing secret"), "{error}");
    }

    #[tokio::test]
    async fn given_a_bridge_close_should_report_its_code_and_leave_the_table() {
        let (url, _server) = bridge().await;
        let sockets = ChatSockets::default();
        let recorder = Arc::new(Recorder::default());
        let id = sockets
            .open(&url, SECRET, "desktop-1", recorder.clone())
            .await
            .unwrap();
        sockets.send(id, "revoke".into()).unwrap();
        let seen = recorder.wait(2).await;
        assert_eq!(
            seen[1],
            ChatSocketEvent::Closed {
                code: Some(4001),
                reason: "Device trust revoked; re-pair deliberately".into()
            }
        );
        for _ in 0..50 {
            if sockets.is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(sockets.is_empty());
    }

    #[tokio::test]
    async fn given_a_failed_channel_send_should_close_going_away_and_leave_the_table() {
        let (url, server) = bridge().await;
        let sockets = ChatSockets::default();
        let gone = Arc::new(Gone(std::sync::atomic::AtomicUsize::new(0)));
        // The bridge's first record (the editor offer) cannot be delivered.
        let id = sockets
            .open(&url, SECRET, "desktop-1", gone.clone())
            .await
            .unwrap();
        assert_eq!(server.await.unwrap(), Some(CloseCode::Away));
        until_empty(&sockets).await;
        assert!(sockets.is_empty());
        assert!(sockets.send(id, "late".into()).is_err());
        assert_eq!(gone.0.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn given_a_reload_should_close_every_socket() {
        let (url, server) = bridge().await;
        let sockets = ChatSockets::default();
        let recorder = Arc::new(Recorder::default());
        let id = sockets
            .open(&url, SECRET, "desktop-1", recorder.clone())
            .await
            .unwrap();
        recorder.wait(1).await;
        assert_eq!(sockets.clear(), 1);
        assert!(sockets.is_empty());
        assert_eq!(server.await.unwrap(), Some(CloseCode::Away));
        assert!(sockets.send(id, "late".into()).is_err());
        assert_eq!(sockets.clear(), 0);
    }
}

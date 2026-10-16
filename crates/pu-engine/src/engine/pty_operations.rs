use std::collections::HashMap;

#[derive(Default)]
pub(super) struct SizeState {
    next: u64,
    owner: Option<u64>,
    claims: HashMap<u64, SizeClaim>,
    tx: Option<tokio::sync::broadcast::Sender<PtySize>>,
    current: PtySize,
}

pub(crate) type PtySize = (u16, u16, Option<String>);

impl SizeState {
    fn publish(&mut self, next: PtySize) {
        if self.current != next {
            self.current = next.clone();
            let _ = self.tx.as_ref().unwrap().send(next);
        }
    }
}

struct SizeClaim {
    client_id: Option<String>,
    desired: Option<(u16, u16)>,
    claimed_at: u64,
}

use std::os::fd::OwnedFd;
use std::sync::Arc;

use pu_core::paths;
use pu_core::protocol::Response;

use crate::delivery::{DeliveryError, DeliveryOptions, deliver_prompt};
use crate::output_buffer::OutputBuffer;

use super::Engine;

impl Engine {
    pub(crate) async fn register_attachment(
        &self,
        agent_id: &str,
        client_id: Option<String>,
    ) -> (u64, tokio::sync::broadcast::Receiver<PtySize>) {
        let mut sizes = self.sizes.lock().await;
        let state = sizes.entry(agent_id.to_string()).or_default();
        state.next += 1;
        let id = state.next;
        state.claims.insert(
            id,
            SizeClaim {
                client_id,
                desired: None,
                claimed_at: 0,
            },
        );
        if state.tx.is_none() {
            let (rows, cols) = self
                .sessions
                .lock()
                .await
                .get(agent_id)
                .map(|h| h.output_buffer.terminal_size())
                .unwrap_or((40, 120));
            state.current = (cols, rows, None);
            state.tx = Some(tokio::sync::broadcast::channel(128).0);
        }
        (id, state.tx.as_ref().unwrap().subscribe())
    }

    /// Serialize the initial screen/offset and size report with ownership changes.
    pub(crate) async fn attachment_replay(
        &self,
        agent_id: &str,
        buffer: &OutputBuffer,
        snapshot: bool,
        size_rx: &mut tokio::sync::broadcast::Receiver<PtySize>,
    ) -> (Vec<u8>, usize, (u16, u16, Option<String>)) {
        let sizes = self.sizes.lock().await;
        let (data, offset) = if snapshot {
            buffer.attach_snapshot()
        } else {
            let (data, offset, _) = buffer.read_from(0);
            (data, offset)
        };
        // The snapshot includes all size changes up to this capture.
        while let Ok(_) | Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) =
            size_rx.try_recv()
        {}
        let size = sizes.get(agent_id).unwrap().current.clone();
        (data, offset, size)
    }

    #[cfg(test)]
    pub(crate) async fn attachment_size(&self, agent_id: &str) -> PtySize {
        self.sizes
            .lock()
            .await
            .get(agent_id)
            .unwrap()
            .current
            .clone()
    }

    pub(crate) async fn attachment_resize(
        &self,
        agent_id: &str,
        id: u64,
        cols: u16,
        rows: u16,
        explicit: bool,
    ) {
        let mut sizes = self.sizes.lock().await;
        let Some(state) = sizes.get_mut(agent_id) else {
            return;
        };
        let Some(claim) = state.claims.get_mut(&id) else {
            return;
        };
        let dims = (cols.max(1), rows.max(1));
        claim.desired = Some(dims);
        let claiming = explicit || claim.client_id.is_none();
        if !claiming && state.owner != Some(id) {
            return;
        }
        if !matches!(
            self.handle_resize(agent_id, dims.0, dims.1).await,
            Response::Ok
        ) {
            return;
        }
        if claiming {
            state.next += 1;
            state.claims.get_mut(&id).unwrap().claimed_at = state.next;
            state.owner = Some(id);
        }
        let owner = state.claims[&id].client_id.clone();
        state.publish((dims.0, dims.1, owner));
    }

    pub(crate) async fn unregister_attachment(&self, agent_id: &str, id: u64) {
        let mut sizes = self.sizes.lock().await;
        let Some(state) = sizes.get_mut(agent_id) else {
            return;
        };
        state.claims.remove(&id);
        if state.claims.is_empty() {
            sizes.remove(agent_id);
            return;
        }
        if state.owner != Some(id) {
            return;
        }
        let next = state
            .claims
            .iter()
            .filter(|(_, c)| c.desired.is_some() && c.claimed_at > 0)
            .max_by_key(|(_, c)| c.claimed_at)
            .map(|(id, c)| (*id, c.client_id.clone(), c.desired.unwrap()));
        state.owner = None;
        if let Some((next_id, owner, (cols, rows))) = next
            && matches!(self.handle_resize(agent_id, cols, rows).await, Response::Ok)
        {
            state.owner = Some(next_id);
            state.publish((cols, rows, owner));
        }
        if state.owner.is_none() {
            state.publish((state.current.0, state.current.1, None));
        }
    }

    pub(super) async fn handle_logs(&self, agent_id: &str, tail: usize) -> Response {
        let buf = {
            let sessions = self.sessions.lock().await;
            match sessions.get(agent_id) {
                Some(handle) => handle.output_buffer.clone(),
                None => return Self::agent_not_found(agent_id),
            }
        };
        let data = buf.read_tail(tail);
        let text = String::from_utf8_lossy(&data);
        if let std::borrow::Cow::Owned(_) = &text {
            tracing::warn!(
                agent_id,
                "logs output contained non-UTF-8 bytes (lossy conversion applied)"
            );
        }
        Response::LogsResult {
            agent_id: agent_id.to_string(),
            data: text.into_owned(),
        }
    }

    pub(super) async fn handle_attach(&self, agent_id: &str) -> Response {
        let sessions = self.sessions.lock().await;
        match sessions.get(agent_id) {
            Some(handle) => Response::AttachReady {
                buffered_bytes: handle.output_buffer.len(),
            },
            None => Self::agent_not_found(agent_id),
        }
    }

    pub(super) async fn handle_input(&self, agent_id: &str, data: &[u8], submit: bool) -> Response {
        if submit {
            let text = String::from_utf8_lossy(data);
            return match self.deliver_text(agent_id, &text).await {
                Ok(true) => Response::Ok,
                Ok(false) => Self::agent_not_found(agent_id),
                Err(DeliveryError::Io(e)) => Response::Error {
                    code: "IO_ERROR".into(),
                    message: format!("write failed: {e}"),
                },
                Err(e) => Response::Error {
                    code: "DELIVERY_FAILED".into(),
                    message: format!("text was not delivered to {agent_id}: {e}"),
                },
            };
        }
        // Clone the fd Arc under the lock, then drop the lock before the blocking write
        let master_fd = {
            let sessions = self.sessions.lock().await;
            match sessions.get(agent_id) {
                Some(handle) => handle.master_fd(),
                None => return Self::agent_not_found(agent_id),
            }
        };
        match self.pty_host.write_to_fd(&master_fd, data).await {
            Ok(()) => Response::Ok,
            Err(e) => Response::Error {
                code: "IO_ERROR".into(),
                message: format!("write failed: {e}"),
            },
        }
    }

    /// Type `text` into an agent and submit it as one turn. Every text
    /// injection (`pu send`, spawn prompts, triggers) goes through here.
    ///
    /// Agents with a readable input box (Claude) get screen-confirmed delivery
    /// and a loud error if it fails; others get the plain chunked write.
    /// Returns `Ok(false)` if the agent has no session.
    pub(super) async fn deliver_text(
        &self,
        agent_id: &str,
        text: &str,
    ) -> Result<bool, DeliveryError> {
        // Serialize per agent so two senders can't interleave keystrokes.
        let lock = {
            let mut locks = self.delivery_locks.lock().await;
            locks.entry(agent_id.to_string()).or_default().clone()
        };
        let _guard = lock.lock().await;

        let (master_fd, output, screen_aware) = {
            let sessions = self.sessions.lock().await;
            match sessions.get(agent_id) {
                Some(h) => (h.master_fd(), h.output_buffer.clone(), h.screen_aware),
                None => return Ok(false),
            }
        };
        if screen_aware {
            deliver_prompt(
                &self.pty_host,
                &master_fd,
                &output,
                text,
                &DeliveryOptions::default(),
            )
            .await?;
        } else {
            self.pty_host
                .write_chunked_submit(&master_fd, text.as_bytes())
                .await?;
        }
        Ok(true)
    }

    pub(crate) async fn handle_legacy_resize(
        &self,
        agent_id: &str,
        cols: u16,
        rows: u16,
    ) -> Response {
        let mut sizes = self.sizes.lock().await;
        let response = self.handle_resize(agent_id, cols.max(1), rows.max(1)).await;
        if matches!(response, Response::Ok)
            && let Some(state) = sizes.get_mut(agent_id)
        {
            state.owner = None;
            state.publish((cols.max(1), rows.max(1), None));
        }
        response
    }

    pub(super) async fn handle_resize(&self, agent_id: &str, cols: u16, rows: u16) -> Response {
        let (master_fd, buffer) = {
            let sessions = self.sessions.lock().await;
            match sessions.get(agent_id) {
                Some(handle) => (handle.master_fd(), handle.output_buffer.clone()),
                None => return Self::agent_not_found(agent_id),
            }
        };
        let result = tokio::task::spawn_blocking(move || {
            buffer.resize_transaction(rows, cols, || {
                use std::os::fd::AsRawFd;
                let size = libc::winsize {
                    ws_row: rows,
                    ws_col: cols,
                    ws_xpixel: 0,
                    ws_ypixel: 0,
                };
                if unsafe { libc::ioctl(master_fd.as_raw_fd(), libc::TIOCSWINSZ, &size) } < 0 {
                    Err(std::io::Error::last_os_error())
                } else {
                    Ok(())
                }
            })
        })
        .await
        .unwrap_or_else(|e| Err(std::io::Error::other(e)));
        match result {
            Ok(()) => Response::Ok,
            Err(e) => Response::Error {
                code: "IO_ERROR".into(),
                message: format!("resize failed: {e}"),
            },
        }
    }

    /// Write data to a PTY fd via the pty host (avoids duplicating unsafe write logic).
    pub async fn write_to_pty(&self, fd: &Arc<OwnedFd>, data: &[u8]) -> Result<(), std::io::Error> {
        self.pty_host.write_to_fd(fd, data).await
    }

    /// Resize a PTY fd via the pty host (avoids duplicating unsafe ioctl logic).
    pub async fn resize_pty(
        &self,
        fd: &Arc<OwnedFd>,
        cols: u16,
        rows: u16,
    ) -> Result<(), std::io::Error> {
        self.pty_host.resize_fd(fd, cols, rows).await
    }

    /// Return the output buffer, master PTY fd, and exit receiver for an agent,
    /// if it has an active session.
    pub async fn get_attach_handles(
        &self,
        agent_id: &str,
    ) -> Option<(
        Arc<OutputBuffer>,
        Arc<OwnedFd>,
        tokio::sync::watch::Receiver<Option<i32>>,
    )> {
        let sessions = self.sessions.lock().await;
        sessions
            .get(agent_id)
            .map(|h| (h.output_buffer.clone(), h.master_fd(), h.exit_rx.clone()))
    }

    /// Build the full environment for spawned agents.
    /// Starts from the user's login shell env, then overrides PATH
    /// (prepends ~/.pu/bin + fallback dirs), TERM, and COLORTERM.
    pub(super) async fn agent_env(&self) -> Vec<(String, String)> {
        let login_env = self.login_env.get_or_init(Self::resolve_login_env).await;
        let mut env = login_env.clone();

        // Extract login PATH for augmentation
        let login_path = env
            .iter()
            .find(|(k, _)| k == "PATH")
            .map(|(_, v)| v.clone())
            .unwrap_or_default();

        // Append common fallback dirs (guards against missing-binary issues)
        let home = std::env::var("HOME").unwrap_or_default();
        let fallbacks = [
            format!("{home}/.local/bin"),
            format!("{home}/.cargo/bin"),
            "/usr/local/bin".to_string(),
            "/opt/homebrew/bin".to_string(),
        ];
        let mut path = login_path;
        for dir in fallbacks {
            if !path.split(':').any(|p| p == dir) {
                path = format!("{path}:{dir}");
            }
        }
        // Prepend ~/.pu/bin
        if let Ok(pu_dir) = paths::global_pu_dir() {
            path = format!("{}:{}", pu_dir.join("bin").display(), path);
        }

        // Override PATH, TERM, COLORTERM in the env
        env.retain(|(k, _)| k != "PATH" && k != "TERM" && k != "COLORTERM");
        env.push(("PATH".into(), path));
        env.push(("TERM".into(), "xterm-256color".into()));
        env.push(("COLORTERM".into(), "truecolor".into()));

        env
    }
}

#[cfg(test)]
mod tests {
    use pu_core::protocol::Request;
    use tokio::io::AsyncWriteExt;

    #[tokio::test]
    async fn given_concurrent_claims_and_snapshot_should_pair_screen_with_matching_geometry() {
        let (engine, id, _tmp) = crate::test_helpers::init_and_spawn().await;
        let engine = std::sync::Arc::new(engine);
        let (_, mut sizes) = engine.register_attachment(&id, Some("viewer".into())).await;
        let (buffer, _, _) = engine.get_attach_handles(&id).await.unwrap();
        let writer_engine = engine.clone();
        let writer_buffer = buffer.clone();
        let writer_id = id.clone();
        let task = tokio::spawn(async move {
            for n in 0..50 {
                // Same serialization boundary as an applied claim; redraw is
                // deliberately immediate after ioctl to stress snapshot capture.
                let mut states = writer_engine.sizes.lock().await;
                let (cols, rows) = if n % 2 == 0 { (80, 24) } else { (120, 40) };
                writer_engine.handle_resize(&writer_id, cols, rows).await;
                writer_buffer.write(b"\x1b[999;999HZ");
                states
                    .get_mut(&writer_id)
                    .unwrap()
                    .publish((cols, rows, Some("owner".into())));
            }
        });
        // Wait for at least one completed redraw before racing subsequent ones.
        sizes.recv().await.unwrap();
        for _ in 0..50 {
            let (data, _, (cols, rows, _)) = engine
                .attachment_replay(&id, &buffer, true, &mut sizes)
                .await;
            let mut parser = vt100::Parser::new(rows, cols, 0);
            parser.process(&data);
            assert_eq!(
                parser.screen().cell(rows - 1, cols - 1).unwrap().contents(),
                "Z"
            );
            tokio::task::yield_now().await;
        }
        task.await.unwrap();
    }

    #[tokio::test]
    async fn given_disconnected_prompt_waiting_for_readiness_should_never_type_when_box_appears() {
        let engine = super::Engine::new();
        let id = "cancel-test".to_string();
        let tmp = tempfile::TempDir::new().unwrap();
        let mut cat = crate::pty_manager::NativePtyHost::new()
            .spawn(crate::pty_manager::SpawnConfig {
                command: "/bin/cat".into(),
                args: vec![],
                cwd: tmp.path().display().to_string(),
                env: vec![],
                env_remove: vec![],
                cols: 120,
                rows: 40,
            })
            .await
            .unwrap();
        cat.screen_aware = true;
        engine.sessions.lock().await.insert(id.clone(), cat);
        let (buffer, _, _) = engine.get_attach_handles(&id).await.unwrap();
        let socket = tmp.path().join("cancel.sock");
        let server = crate::ipc_server::IpcServer::bind(&socket, engine).unwrap();
        let server_task = tokio::spawn(server.run());
        let mut client = tokio::net::UnixStream::connect(&socket).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        buffer.write(b"\x1b[2J\x1b[Hloading");
        let request = Request::Input {
            agent_id: id,
            data: b"MUST_NOT_TYPE".to_vec(),
            submit: true,
            cancel_on_disconnect: true,
        };
        client
            .write_all(format!("{}\n", serde_json::to_string(&request).unwrap()).as_bytes())
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(80)).await;
        drop(client);
        tokio::time::sleep(std::time::Duration::from_millis(80)).await;
        buffer.write("\x1b[2J\x1b[H────────\r\n❯ \r\n────────\r\n".as_bytes());
        let offset = buffer.current_offset();
        // Longer than readiness settle and Enter's pre-submit delay.
        tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
        assert_eq!(
            buffer.current_offset(),
            offset,
            "canceled prompt wrote after readiness returned"
        );
        server_task.abort();
    }
}

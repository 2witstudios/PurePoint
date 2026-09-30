use std::os::fd::OwnedFd;
use std::sync::Arc;

use pu_core::paths;
use pu_core::protocol::Response;

use crate::delivery::{DeliveryError, DeliveryOptions, deliver_prompt};
use crate::output_buffer::OutputBuffer;

use super::Engine;

impl Engine {
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

    pub(super) async fn handle_resize(&self, agent_id: &str, cols: u16, rows: u16) -> Response {
        // Clone the fd Arc under the lock, then drop the lock before the blocking ioctl
        let master_fd = {
            let sessions = self.sessions.lock().await;
            match sessions.get(agent_id) {
                Some(handle) => handle.master_fd(),
                None => return Self::agent_not_found(agent_id),
            }
        };
        match self.pty_host.resize_fd(&master_fd, cols, rows).await {
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

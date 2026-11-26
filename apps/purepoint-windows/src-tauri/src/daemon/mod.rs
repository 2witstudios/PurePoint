//! Daemon client and lifecycle: find, start and talk to `pu-engine` (WIN-5.3).
//!
//! - `client`: NDJSON one-shot requests and long-lived connections (`DaemonClient.swift`).
//! - `lifecycle`: find, reuse, start and restart the local daemon (`DaemonLifecycle.swift`).
//! - `relay`: a machine's daemon over `ssh.exe … pu daemon proxy`, for remote projects.
//!
//! Tauri surface (front end: `ui/src/services/daemon.ts`): commands `daemon_request`,
//! `daemon_health` and `daemon_status`, and the `daemon-state` event carrying a
//! [`lifecycle::ConnectionState`] on every change.

pub mod client;
pub mod lifecycle;
pub mod relay;

use std::sync::Arc;
use std::time::Duration;

use pu_core::protocol::{Request, Response};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::git::host::{GitExec, HostError};
use client::{DaemonClient, DaemonError};
use lifecycle::{BinaryLocator, ConnectionState, DaemonLifecycle};

/// Event emitted with the new [`ConnectionState`] whenever it changes.
pub const DAEMON_STATE_EVENT: &str = "daemon-state";

/// Tauri state: the local daemon's lifecycle, or why it could not be set up.
pub struct DaemonState {
    lifecycle: Result<Arc<DaemonLifecycle>, String>,
}

impl DaemonState {
    /// The lifecycle for `%USERPROFILE%\.pu\daemon.sock`, emitting `daemon-state`.
    pub fn for_app(app: &AppHandle) -> Self {
        let lifecycle = pu_core::paths::daemon_socket_path()
            .and_then(|socket| {
                let locator = BinaryLocator::for_app(app.path().resource_dir().ok());
                DaemonLifecycle::new(&socket, locator)
            })
            .map(Arc::new)
            .map_err(|e| format!("Could not locate the daemon socket: {e}"));
        if let Ok(lifecycle) = &lifecycle {
            let app = app.clone();
            lifecycle.on_change(move |state| {
                let _ = app.emit(DAEMON_STATE_EVENT, state);
            });
        }
        Self { lifecycle }
    }

    /// The lifecycle, for other backends that restart the daemon (the installer).
    pub(crate) fn lifecycle(&self) -> Result<Arc<DaemonLifecycle>, DaemonError> {
        self.lifecycle.clone().map_err(DaemonError::Io)
    }

    /// Start (or reuse) the daemon in the background, as the Mac does at launch.
    pub fn start(&self) {
        if let Ok(lifecycle) = self.lifecycle.clone() {
            tauri::async_runtime::spawn(async move {
                let _ = lifecycle.ensure_daemon().await;
            });
        }
    }
}

/// Send one request to the local daemon. A daemon found gone is started again and the
/// request retried once.
pub async fn send(lifecycle: &DaemonLifecycle, request: &Request) -> Result<Response, DaemonError> {
    match lifecycle.client().request(request).await {
        Err(error @ DaemonError::NotRunning(_)) => {
            lifecycle.note_failure(&error);
            lifecycle.ensure_daemon().await?;
            lifecycle.client().request(request).await
        }
        other => other,
    }
}

fn to_json(response: Response) -> Result<serde_json::Value, DaemonError> {
    serde_json::to_value(response)
        .map_err(|e| DaemonError::Io(format!("Could not encode the reply: {e}")))
}

/// One request to the local daemon: `json` is a protocol `Request`, the reply a `Response`.
#[tauri::command]
pub async fn daemon_request(
    state: State<'_, DaemonState>,
    json: serde_json::Value,
) -> Result<serde_json::Value, DaemonError> {
    let request: Request =
        serde_json::from_value(json).map_err(|e| DaemonError::InvalidRequest(e.to_string()))?;
    let lifecycle = state.lifecycle()?;
    to_json(send(&lifecycle, &request).await?)
}

/// The daemon's `health_report`, starting the daemon if none is running.
#[tauri::command]
pub async fn daemon_health(
    state: State<'_, DaemonState>,
) -> Result<serde_json::Value, DaemonError> {
    let lifecycle = state.lifecycle()?;
    lifecycle.ensure_daemon().await?;
    to_json(lifecycle.health().await?)
}

/// The current connection state (the last `daemon-state` payload).
#[tauri::command]
pub fn daemon_status(state: State<'_, DaemonState>) -> ConnectionState {
    match &state.lifecycle {
        Ok(lifecycle) => lifecycle.state(),
        Err(message) => ConnectionState::Failed {
            kind: "io".into(),
            message: message.clone(),
        },
    }
}

/// [`GitExec`] over a daemon: remote projects' `git_exec` and `fs_*` requests.
pub struct DaemonGitExec {
    client: DaemonClient,
    runtime: tokio::runtime::Handle,
}

impl DaemonGitExec {
    pub fn new(client: DaemonClient, runtime: tokio::runtime::Handle) -> Self {
        Self { client, runtime }
    }

    /// A machine's daemon, reached through `ssh.exe` on Tauri's runtime.
    pub fn for_machine(machine_id: &str) -> Self {
        Self::new(
            DaemonClient::new(Arc::new(relay::SshRelayConnector::new(machine_id))),
            tauri::async_runtime::handle().inner().clone(),
        )
    }
}

impl GitExec for DaemonGitExec {
    /// Blocks; callers run on the blocking pool, never on the async runtime.
    fn send(&self, request: Request, timeout: Duration) -> Result<Response, HostError> {
        self.runtime
            .block_on(self.client.request_with_timeout(&request, timeout))
            .map_err(|error| match error {
                DaemonError::NotRunning(_) => HostError::unavailable(),
                DaemonError::Busy(_) => HostError::Daemon {
                    code: "BUSY".into(),
                    message: error.to_string(),
                },
                other => HostError::Daemon {
                    code: other.kind().into(),
                    message: other.to_string(),
                },
            })
    }
}

#[cfg(test)]
mod tests {
    use super::client::tests::{Reply, fake_daemon};
    use super::*;
    use crate::git::host::{DaemonProjectHost, ProjectHost};
    use crate::git::runner::HostCommand;

    #[test]
    fn given_daemon_git_exec_should_run_git_through_the_daemon() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let tmp = tempfile::TempDir::new().unwrap();
        let socket = tmp.path().join("d.sock");
        let reply = r#"{"type":"git_exec_result","exit_code":0,"stdout_base64":"bWFpbgo=","stderr_base64":"","truncated":false}"#;
        let server = {
            let _entered = runtime.enter();
            fake_daemon(&socket, Reply::Line(reply.into()))
        };
        let exec = DaemonGitExec::new(
            DaemonClient::local(&socket).unwrap(),
            runtime.handle().clone(),
        );
        let host = DaemonProjectHost::new(exec, "m");
        let command = HostCommand {
            program: "git".into(),
            executable_path: "git".into(),
            args: vec!["branch".into(), "--show-current".into()],
            cwd: "/repo".into(),
            ..HostCommand::default()
        };
        let result = host.run(&command);
        assert_eq!((result.exit_code, result.stdout.as_str()), (0, "main\n"));

        server.abort();
        let gone = DaemonGitExec::new(
            DaemonClient::local(&tmp.path().join("none.sock")).unwrap(),
            runtime.handle().clone(),
        );
        assert_eq!(
            gone.send(Request::Health, Duration::from_secs(5))
                .unwrap_err(),
            HostError::unavailable()
        );
    }
}

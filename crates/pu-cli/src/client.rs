use std::path::Path;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

use pu_core::protocol::{Request, Response};

use crate::error::CliError;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Requests that type a prompt into an agent wait on the daemon's
/// screen-confirmed delivery, which can take up to ~170s (60s for the input
/// box, three typing attempts, submit confirmation). Waiting longer than that
/// lets the daemon's own error (e.g. "input box never became ready") reach
/// the user instead of a bare timeout.
const DELIVERY_TIMEOUT: Duration = Duration::from_secs(180);

fn timeout_for(request: &Request) -> Duration {
    match request {
        Request::Input { submit: true, .. } => DELIVERY_TIMEOUT,
        // Plan mode prefixes its own instructions, so even a blank prompt is
        // delivered (the engine only rejects a literally empty one).
        Request::Spawn {
            prompt, plan_mode, ..
        } if *plan_mode || !prompt.trim().is_empty() => DELIVERY_TIMEOUT,
        _ => REQUEST_TIMEOUT,
    }
}

pub async fn send_request(socket: &Path, request: &Request) -> Result<Response, CliError> {
    // Session operations are already global. An explicit project constrains them.
    if let Some(project_root) = crate::commands::explicit_project() {
        let id = match request {
            Request::Logs { agent_id, .. }
            | Request::Input { agent_id, .. }
            | Request::Attach { agent_id }
            | Request::Resize { agent_id, .. } => Some(agent_id),
            _ => None,
        };
        if let Some(id) = id {
            let response = send_raw_request(
                socket,
                &Request::ResolveAgent {
                    agent_id: id.clone(),
                    project_root: Some(project_root),
                },
            )
            .await?;
            if !matches!(response, Response::AgentResolved { .. }) {
                return Ok(response);
            }
        }
    }
    send_raw_request(socket, request).await
}

async fn send_raw_request(socket: &Path, request: &Request) -> Result<Response, CliError> {
    let timeout = timeout_for(request);
    let result = tokio::time::timeout(timeout, async {
        let stream = UnixStream::connect(socket)
            .await
            .map_err(|e| match e.kind() {
                std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::NotFound => {
                    CliError::DaemonNotRunning
                }
                _ => CliError::Io(e),
            })?;
        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);

        let json = serde_json::to_string(request)?;
        writer
            .write_all(json.as_bytes())
            .await
            .map_err(CliError::Io)?;
        writer.write_all(b"\n").await.map_err(CliError::Io)?;

        let mut line = String::new();
        reader.read_line(&mut line).await.map_err(CliError::Io)?;
        let response: Response = serde_json::from_str(line.trim())?;
        Ok::<Response, CliError>(response)
    })
    .await;

    match result {
        Ok(inner) => inner,
        Err(_) => Err(CliError::RequestTimeout(timeout.as_secs())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pu_core::protocol::{PROTOCOL_VERSION, Request, Response};
    use tempfile::TempDir;

    // Helper: start a real pu-engine IPC server for testing
    async fn start_test_server(sock_path: &std::path::Path) -> tokio::task::JoinHandle<()> {
        let engine = pu_engine::engine::Engine::new();
        let server = pu_engine::ipc_server::IpcServer::bind(sock_path, engine).unwrap();
        tokio::spawn(async move {
            server.run().await.ok();
        })
    }

    #[test]
    fn given_submitted_input_should_wait_for_delivery() {
        let input = |submit| Request::Input {
            agent_id: "ag-1".into(),
            data: b"hi".to_vec(),
            submit,
        };
        assert_eq!(timeout_for(&input(true)), DELIVERY_TIMEOUT);
        assert_eq!(timeout_for(&input(false)), REQUEST_TIMEOUT);
        assert_eq!(timeout_for(&Request::Health), REQUEST_TIMEOUT);
    }

    #[test]
    fn given_prompted_or_plan_mode_spawn_should_wait_for_delivery() {
        let spawn = |prompt: &str, plan_mode| Request::Spawn {
            project_root: "/p".into(),
            prompt: prompt.into(),
            agent: "claude".into(),
            name: None,
            base: None,
            root: false,
            worktree: None,
            command: None,
            no_auto: false,
            extra_args: vec![],
            plan_mode,
            no_trigger: false,
            trigger: None,
        };
        assert_eq!(timeout_for(&spawn("review", false)), DELIVERY_TIMEOUT);
        assert_eq!(timeout_for(&spawn("  ", true)), DELIVERY_TIMEOUT);
        assert_eq!(timeout_for(&spawn("  ", false)), REQUEST_TIMEOUT);
        assert_eq!(timeout_for(&spawn("", false)), REQUEST_TIMEOUT);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn given_running_daemon_should_send_and_receive() {
        let tmp = TempDir::new().unwrap();
        let sock = tmp.path().join("test.sock");
        let handle = start_test_server(&sock).await;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let resp = send_request(&sock, &Request::Health).await.unwrap();
        match resp {
            Response::HealthReport {
                protocol_version, ..
            } => {
                assert_eq!(protocol_version, PROTOCOL_VERSION);
            }
            other => panic!("expected HealthReport, got {other:?}"),
        }

        // Cleanup
        send_request(&sock, &Request::Shutdown).await.ok();
        handle.await.ok();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn given_no_daemon_should_return_error() {
        let tmp = TempDir::new().unwrap();
        let sock = tmp.path().join("nonexistent.sock");
        let result = send_request(&sock, &Request::Health).await;
        assert!(result.is_err());
    }
}

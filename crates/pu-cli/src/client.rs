use std::path::Path;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

use pu_core::protocol::{Request, Response};

use crate::error::CliError;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

pub async fn send_request(socket: &Path, request: &Request) -> Result<Response, CliError> {
    let result = tokio::time::timeout(REQUEST_TIMEOUT, async {
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
            .write_all(format!("{json}\n").as_bytes())
            .await
            .map_err(CliError::Io)?;

        let mut line = String::new();
        reader.read_line(&mut line).await.map_err(CliError::Io)?;
        let response: Response = serde_json::from_str(line.trim())?;
        Ok::<Response, CliError>(response)
    })
    .await;

    match result {
        Ok(inner) => inner,
        Err(_) => Err(CliError::RequestTimeout),
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

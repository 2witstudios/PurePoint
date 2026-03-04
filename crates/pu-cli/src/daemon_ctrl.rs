use std::path::{Path, PathBuf};

use pu_core::paths;
use pu_core::protocol::{Request, Response};

use crate::error::CliError;

pub fn find_daemon_binary() -> Option<PathBuf> {
    which::which("pu-engine").ok()
}

pub async fn check_daemon_health(socket: &Path) -> bool {
    matches!(
        crate::client::send_request(socket, &Request::Health).await,
        Ok(Response::HealthReport { .. })
    )
}

pub async fn ensure_daemon(socket: &Path) -> Result<(), CliError> {
    if check_daemon_health(socket).await {
        return Ok(());
    }

    let binary = find_daemon_binary().ok_or(CliError::Other(
        "pu-engine not found on PATH — install with `cargo install --path crates/pu-engine`".into(),
    ))?;

    // Redirect daemon stderr to log file so startup errors are diagnosable
    let stderr_target = match paths::daemon_log_path() {
        Ok(log_path) => {
            if let Some(parent) = log_path.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            match std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log_path)
            {
                Ok(file) => std::process::Stdio::from(file),
                Err(_) => std::process::Stdio::null(),
            }
        }
        Err(_) => std::process::Stdio::null(),
    };

    // Start daemon
    std::process::Command::new(&binary)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(stderr_target)
        .spawn()
        .map_err(CliError::Io)?;

    // Poll for socket with exponential backoff: 10, 20, 40, 80, 160, 320, 640ms
    let mut delay_ms = 10u64;
    let mut total_ms = 0u64;
    while total_ms < 3000 {
        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
        total_ms += delay_ms;
        if check_daemon_health(socket).await {
            return Ok(());
        }
        delay_ms = (delay_ms * 2).min(640);
    }

    Err(CliError::Other(
        "daemon did not start within 3 seconds".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_find_daemon_binary_should_return_path_or_none() {
        let result = find_daemon_binary();
        // If found, verify the path is an executable file
        if let Some(path) = result {
            assert!(path.exists(), "found binary does not exist: {path:?}");
            assert!(
                path.to_string_lossy().contains("pu-engine"),
                "binary path should contain 'pu-engine': {path:?}"
            );
        }
        // If not found, that's fine — just verify no panic
    }

    #[tokio::test(flavor = "current_thread")]
    async fn given_running_daemon_should_report_healthy() {
        use tempfile::TempDir;
        let tmp = TempDir::new().unwrap();
        let sock = tmp.path().join("test.sock");

        let engine = pu_engine::engine::Engine::new();
        let server = pu_engine::ipc_server::IpcServer::bind(&sock, engine).unwrap();
        let handle = tokio::spawn(async move {
            server.run().await.ok();
        });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let healthy = check_daemon_health(&sock).await;
        assert!(healthy);

        crate::client::send_request(&sock, &pu_core::protocol::Request::Shutdown)
            .await
            .ok();
        handle.await.ok();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn given_no_daemon_should_report_not_healthy() {
        use tempfile::TempDir;
        let tmp = TempDir::new().unwrap();
        let sock = tmp.path().join("nope.sock");
        let healthy = check_daemon_health(&sock).await;
        assert!(!healthy);
    }
}

use std::path::PathBuf;

use pu_core::paths;
use pu_engine::daemon_lifecycle;
use pu_engine::engine::Engine;
use pu_engine::ipc_server::IpcServer;

#[tokio::main]
async fn main() {
    // Parse args
    let args: Vec<String> = std::env::args().collect();
    let managed = args.contains(&"--managed".to_string());
    let socket_path = args
        .windows(2)
        .find(|w| w[0] == "--socket")
        .map(|w| PathBuf::from(&w[1]));

    let socket = socket_path.unwrap_or_else(|| {
        paths::daemon_socket_path().unwrap_or_else(|e| {
            eprintln!("failed to resolve socket path: {e}");
            std::process::exit(1);
        })
    });
    let pid_path = paths::daemon_pid_path().unwrap_or_else(|e| {
        eprintln!("failed to resolve PID path: {e}");
        std::process::exit(1);
    });

    // Setup tracing
    tracing_subscriber::fmt()
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();

    // Create global dir
    if let Some(parent) = socket.parent() {
        std::fs::create_dir_all(parent).ok();
    }

    // Write PID file (standalone mode only)
    if !managed
        && let Err(e) = daemon_lifecycle::write_pid_file(&pid_path) {
            eprintln!("failed to write PID file: {e}");
            std::process::exit(1);
        }

    tracing::info!(pid = std::process::id(), socket = %socket.display(), managed, "starting pu-engine");

    // In managed mode, exit when the parent process (macOS app) dies.
    // Without this, the daemon outlives app restarts and stale binaries persist.
    if managed {
        let parent_pid = std::os::unix::process::parent_id();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                // On macOS, orphaned processes get reparented to PID 1 (launchd)
                if std::os::unix::process::parent_id() != parent_pid {
                    tracing::info!("parent process died, shutting down managed daemon");
                    std::process::exit(0);
                }
            }
        });
    }

    let engine = Engine::new().await;
    let server = match IpcServer::bind(&socket, engine) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("failed to bind socket: {e}");
            std::process::exit(1);
        }
    };

    if let Err(e) = server.run().await {
        tracing::error!("server error: {e}");
    }

    // Cleanup
    if !managed {
        daemon_lifecycle::cleanup_files(&pid_path, &socket);
    }

    tracing::info!("pu-engine stopped");
}

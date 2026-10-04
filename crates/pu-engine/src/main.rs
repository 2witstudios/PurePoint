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
    let pid_path = daemon_lifecycle::pid_path_for(&socket);

    // Setup tracing
    tracing_subscriber::fmt()
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();

    // Create global dir
    if let Some(parent) = socket.parent() {
        std::fs::create_dir_all(parent).ok();
    }

    // One daemon per socket, in every mode. Binding unlinks the socket file, so
    // a second daemon (a racing app launch, or the CLI auto-starting one) would
    // steal the path and strand every agent the first one is running.
    let _daemon_lock = match daemon_lifecycle::try_acquire_daemon_lock(&socket) {
        Ok(Some(lock)) => lock,
        Ok(None) => {
            eprintln!(
                "pu-engine already running for {}; exiting",
                socket.display()
            );
            std::process::exit(0);
        }
        Err(e) => {
            eprintln!("failed to take daemon lock: {e}");
            std::process::exit(1);
        }
    };

    // Written in managed mode too, so the app can find and stop a daemon it
    // cannot reach over the socket.
    if let Err(e) = daemon_lifecycle::claim_pid_file(&pid_path) {
        eprintln!("failed to write PID file: {e}");
        std::process::exit(1);
    }

    tracing::info!(pid = std::process::id(), socket = %socket.display(), managed, "starting pu-engine");

    // Apps launched by launchd get a 256-fd soft limit. Every agent holds PTY
    // fds and every client stream holds a socket, so raise it to the hard limit.
    daemon_lifecycle::raise_fd_limit();

    let engine = Engine::new();
    let server = match IpcServer::bind(&socket, engine) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("failed to bind socket: {e}");
            std::process::exit(1);
        }
    };

    // Start background reaper for dead sessions and orphaned channels
    server.engine().start_session_reaper();

    // Start background scheduler for recurring tasks
    server.engine().start_scheduler();

    // In managed mode, exit when the parent process (macOS app) dies.
    // Without this, the daemon outlives app restarts and stale binaries persist.
    // process::exit(0) skips Drop, so explicitly kill agent process groups first
    // — otherwise vitest/node workers and other grandchildren orphan to launchd.
    if managed {
        let parent_pid = std::os::unix::process::parent_id();
        let engine_for_cleanup = server.engine().clone();
        let pid_path_for_exit = pid_path.clone();
        let socket_for_exit = socket.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                // On macOS, orphaned processes get reparented to PID 1 (launchd)
                if std::os::unix::process::parent_id() != parent_pid {
                    tracing::info!("parent process died, shutting down managed daemon");
                    let _ = tokio::time::timeout(
                        std::time::Duration::from_secs(4),
                        engine_for_cleanup.kill_all_sessions(std::time::Duration::from_secs(1)),
                    )
                    .await;
                    daemon_lifecycle::cleanup_files(&pid_path_for_exit, &socket_for_exit);
                    std::process::exit(0);
                }
            }
        });
    }

    let engine = server.engine().clone();
    if let Err(e) = server.run().await {
        tracing::error!("server error: {e}");
    }

    // Stop agents gracefully (SIGTERM, then SIGKILL) while the runtime is still
    // up. They stay resumable: begin_shutdown keeps their exits from being
    // recorded as Broken.
    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        engine.kill_all_sessions(std::time::Duration::from_secs(2)),
    )
    .await;

    daemon_lifecycle::cleanup_files(&pid_path, &socket);

    tracing::info!("pu-engine stopped");
}

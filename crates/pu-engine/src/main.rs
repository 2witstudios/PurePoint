use std::path::PathBuf;

use pu_core::paths;
use pu_engine::daemon_lifecycle;
use pu_engine::engine::Engine;
use pu_engine::ipc_server::IpcServer;

fn main() {
    // Guardian mode runs before any daemon setup: synchronous, no runtime,
    // no tracing, no state directory, no daemon lock.
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--mesh-guardian") {
        std::process::exit(pu_engine::mesh_guardian::run_guardian(&args[2..]));
    }
    daemon_main(args);
}

#[tokio::main]
async fn daemon_main(args: Vec<String>) {
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
    // Setup tracing
    tracing_subscriber::fmt()
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();

    // Validate/create a private state directory before any lifecycle writes.
    // Both startup modes hold the same lock through agent shutdown.
    let mut daemon_lock = match daemon_lifecycle::try_acquire_daemon_lock(&socket) {
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
    if let Err(e) = daemon_lock.claim_pid_file() {
        eprintln!("failed to write PID file: {e}");
        drop(daemon_lock);
        std::process::exit(1);
    }

    tracing::info!(pid = std::process::id(), socket = %socket.display(), managed, "starting pu-engine");

    // Apps launched by launchd get a 256-fd soft limit. Every agent holds PTY
    // fds and every client stream holds a socket, so raise it to the hard limit.
    daemon_lifecycle::raise_fd_limit();

    let engine = match Engine::with_project_registry(paths::project_registry_path(&socket)) {
        Ok(engine) => engine,
        Err(e) => {
            eprintln!("failed to load project registry: {e}");
            drop(daemon_lock);
            std::process::exit(1);
        }
    };
    let server = match IpcServer::bind_with_lock(daemon_lock, engine) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("failed to bind socket: {e}");
            std::process::exit(1);
        }
    };
    let lifecycle = server.lifecycle();
    let work = server.work();

    // Start background reaper for dead sessions and orphaned channels
    server.engine().start_session_reaper();

    // Start background scheduler for recurring tasks
    server.engine().start_scheduler();

    // In managed mode, exit when the parent process (macOS app) dies.
    // Without this, the daemon outlives app restarts and stale binaries persist.
    // Use the same request/service drain and agent teardown as ordinary shutdown.
    if managed {
        let parent_pid = std::os::unix::process::parent_id();
        let work_for_exit = work.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                // On macOS, orphaned processes get reparented to PID 1 (launchd)
                if std::os::unix::process::parent_id() != parent_pid {
                    tracing::info!("parent process died, shutting down managed daemon");
                    work_for_exit.begin_shutdown();
                    break;
                }
            }
        });
    }

    let engine = server.engine().clone();
    if let Err(e) = server.run().await {
        tracing::error!("server error: {e}");
    }
    // A bounded IPC drain can time out without relinquishing ownership. Do not
    // tear down sessions or exit while an admitted spawn/tick can still publish.
    work.wait_requests().await;

    // Stop agents gracefully (SIGTERM, then SIGKILL) while the runtime is still
    // up. They stay resumable: kill_all_sessions keeps the exits it causes from
    // being recorded as Broken.
    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        engine.kill_all_sessions(std::time::Duration::from_secs(2)),
    )
    .await;

    if let Err(error) = work.drain_for(std::time::Duration::from_secs(5)).await {
        tracing::warn!(%error, "shutdown still draining; keeping singleton and runtime artifacts");
        work.wait_idle().await;
    }

    lifecycle.cleanup();

    tracing::info!("pu-engine stopped");

    // Exit now rather than returning: dropping the runtime waits on every
    // blocking PTY reader, and one whose terminal a surviving grandchild still
    // holds open never returns, leaving a socketless daemon lingering.
    std::process::exit(0);
}

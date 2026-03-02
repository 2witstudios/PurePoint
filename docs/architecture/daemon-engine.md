# Daemon Engine

**Maturity: CONVERGING**

## Context

PurePoint coordinates AI coding agents, worktrees, and project state across multiple projects simultaneously. A long-running daemon process is the natural architecture for this: it maintains persistent state, pushes real-time updates to clients, and manages long-lived agent sessions without requiring each CLI invocation to bootstrap from scratch. CLI and desktop app act as thin clients to this daemon. Getting the daemon's process model right is foundational to everything else.

## Decisions

! [DAEMON-001] PID-file with CLI auto-start — simpler than launchd/systemd, cross-platform, no plist maintenance. Daemon writes `~/.pu/daemon.pid` using `create_new(true)` (`O_EXCL`) which fails atomically if another instance is running. CLI auto-starts the daemon via `ensure_daemon()`: spawns `pu-engine` as a detached process (stdin/stdout null, stderr to `~/.pu/daemon.log`), then polls health every 100ms for up to 30 attempts (3s timeout). On timeout, exits with error pointing to `~/.pu/daemon.log`. Implemented in `pu-cli/src/daemon_ctrl.rs`.

! [DAEMON-004] Tokio async runtime with `spawn_blocking` for filesystem and process ops — the daemon is I/O-bound (IPC, PTY reads, file writes), making async the natural fit. Blocking operations (PTY `read`/`write`/`ioctl`, `waitpid`, filesystem) run in `spawn_blocking` to avoid blocking the event loop. Implemented in `pu-engine/src/main.rs` (tokio main), `pu-engine/src/pty_manager.rs` (spawn_blocking for PTY I/O and waitpid).

! [DAEMON-005] `Request::Health` returns `Response::HealthReport` with PID, uptime seconds, protocol version, project list, and agent count — lightweight liveness check used by CLI auto-start and `pu health` command. Implemented in `pu-core/src/protocol.rs` (`HealthReport` variant).

## Open Questions

? [DAEMON-002] How should the daemon handle multi-project state?
The daemon needs a global registry of all known projects, plus per-project state. Should this be a single global store, separate per-project stores, or a hybrid? How does the daemon discover projects — explicit registration, or scan for `.pu/` directories? (Code supports multi-project via `project_root` parameter on requests, but design not fully explored.)

? [DAEMON-003] What is the crash recovery model?
If the daemon crashes, agents may still be running. On restart, the daemon needs to reconcile its state with reality. Should this be automatic on daemon startup? (Not yet implemented.)

? [DAEMON-006] How should the daemon classify incoming tasks for context assembly?
The daemon is the context assembler — every task should automatically inject relevant specs and knowledge into agent prompts. How does the daemon decide what context is relevant? Options: keyword matching, task classification, explicit tags in spawn commands, or template-defined context maps. (Not yet implemented.)

## Design Directions

- Primary platform: macOS. Future: Linux.
- Auto-start when CLI or app needs it and it's not running
- Graceful shutdown with resource cleanup (SIGTERM/SIGINT handled, PID file + socket cleaned up)
- No root/sudo requirement
- Support for multiple projects simultaneously
- Managed mode (`--managed` flag) for when CLI controls the socket path; skips PID file

## Research Notes

**Daemon startup sequence (from `pu-engine/src/main.rs`):** Parse args (looks for `--managed` flag and `--socket <path>`). Resolve socket path (`--socket` arg or `~/.pu/daemon.sock`). Init tracing. Create `Engine`, bind `IpcServer` to socket (removes stale socket file first). In standalone mode, write PID file; in managed mode, skip PID file. Run server until SIGTERM, SIGINT, or `Request::Shutdown`. On exit, clean up PID file and socket.

**Shutdown handling:** SIGTERM and SIGINT are caught via `tokio::signal::unix`. `Request::Shutdown` from any client also triggers graceful shutdown via `Arc<Notify>`.

## Related

- [DAEMON-006] connects to [STORE-006] (spec indexing for retrieval) and [AGENT-005]/[AGENT-006] (context injection into agents) — these are different faces of the same context assembly problem.

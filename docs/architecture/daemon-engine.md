# Daemon Engine

**Maturity: CONVERGING**

## Context

PurePoint coordinates AI coding agents, worktrees, and project state across multiple projects simultaneously. A long-running daemon process is the natural architecture for this: it maintains persistent state, pushes real-time updates to clients, and manages long-lived agent sessions without requiring each CLI invocation to bootstrap from scratch. CLI and desktop app act as thin clients to this daemon. Getting the daemon's process model right is foundational to everything else.

## Decisions

! [DAEMON-001] PID-file with CLI auto-start — simpler than launchd/systemd, cross-platform, no plist maintenance. Single-instance is enforced by an exclusive non-blocking `flock` on `~/.pu/daemon.sock.lock` (beside the socket), held for the daemon's lifetime and released by the kernel however it dies; a daemon that cannot take it exits 0. The PID file `~/.pu/daemon.sock.pid` is then written in both modes (an `O_EXCL` PID file alone was not enough: managed daemons skipped it, so app launches and CLI auto-start raced and each unlinked and rebound the live socket). CLI auto-starts the daemon via `ensure_daemon()`: after three failed health checks 200ms apart, spawns `pu-engine` as a detached process (stdin/stdout null, stderr to `~/.pu/daemon.log`), then polls health with exponential backoff (10, 20, 40, 80, 160, 320, then 640 ms), bounded by a 3 s wall-clock timeout that includes the probes themselves (each capped at 2 s). A `BUSY` reply counts as a live daemon. On timeout, exits with error pointing to `~/.pu/daemon.log`. Implemented in `pu-cli/src/daemon_ctrl.rs`.

! [DAEMON-004] Tokio async runtime with `spawn_blocking` for filesystem and process ops — the daemon is I/O-bound (IPC, PTY reads, file writes), making async the natural fit. Blocking operations (PTY `read`/`write`/`ioctl`, `waitpid`, filesystem) run in `spawn_blocking` to avoid blocking the event loop. Implemented in `pu-engine/src/main.rs` (tokio main), `pu-engine/src/pty_manager.rs` (spawn_blocking for PTY I/O and waitpid).

! [DAEMON-007] Daemon is a separate subprocess, not an in-process library. Evaluated UniFFI (Rust-as-static-library with generated Swift bindings) but rejected: crash isolation requires process boundary (PTY/process crash must not take down the UI), the `pu` CLI also connects to the daemon (shared service, not UI-scoped), and all IPC code (DaemonClient, DaemonAttachSession, streaming protocol) is already built and tested. The subprocess model with embedded binary in the app bundle preserves all these properties.

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
- Graceful shutdown with resource cleanup (SIGTERM/SIGINT handled, agents stopped with SIGTERM then SIGKILL, PID file + socket cleaned up). Agents killed by shutdown are not recorded as `Broken`, so the next daemon's init marks them suspended and they resume.
- No root/sudo requirement
- Support for multiple projects simultaneously
- One daemon per socket in every mode: an exclusive `flock` on `daemon.sock.lock`, taken before the socket is touched. A second daemon (a racing app launch, or the CLI auto-starting one) exits instead of unlinking the live socket and stranding the first daemon's agents.
- Managed mode (`--managed` flag): launched by the macOS app as an embedded subprocess. Writes the PID file like standalone mode, so the app can stop a daemon it cannot reach. Exits when the app dies. App sends Shutdown on quit. Agents stop, state saved to manifest for restore.
- Standalone mode (default): launched by CLI or manually. Agents persist across CLI sessions.

## Research Notes

**Daemon startup sequence (from `pu-engine/src/main.rs`):** Parse args (looks for `--managed` flag and `--socket <path>`). Resolve socket path (`--socket` arg or `~/.pu/daemon.sock`). Init tracing. Take the `daemon.sock.lock` flock (exit 0 if another daemon holds it), then write the PID file (both modes) and raise the fd soft limit. Create `Engine`, bind `IpcServer` to socket (removes the stale socket file first, which is safe only because the lock is held). Run server until SIGTERM, SIGINT, or `Request::Shutdown`. On exit, stop agents gracefully, then remove the PID file and socket if the PID file still names this daemon.

**Shutdown handling:** SIGTERM and SIGINT are caught via `tokio::signal::unix`. `Request::Shutdown` from any client also triggers graceful shutdown via `Arc<Notify>`.

## Related

- [DAEMON-006] connects to [STORE-006] (spec indexing for retrieval) and [AGENT-005]/[AGENT-006] (context injection into agents) — these are different faces of the same context assembly problem.

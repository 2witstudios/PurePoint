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

#### [DAEMON-002] Global visibility for Point Guard

**Researched: 2026-10-10.** The following records the pre-change evidence and design alternatives. Implementation now provides the proposed global summary and project/agent/worktree inventories, explicit CLI scope, durable socket-specific registration, and agent ownership routing. See `product/cli.md` for the specified feature contract. Registry removal/import operations remain design directions rather than shipped commands.

The daemon already has `Engine::registered_projects`, an in-memory set used by the scheduler (`crates/pu-engine/src/engine/mod.rs`, `engine/scheduler.rs`). Selected project-scoped requests insert their supplied root before execution, so unsuccessful requests can register invalid roots. The set starts empty on daemon restart and stores paths without canonicalization. The desktop app separately persists open projects in UserDefaults (`apps/purepoint-macos/purepoint-macos/State/AppState.swift`). Open app projects and daemon-known projects therefore represent different scopes.

Despite the existing HealthReport schema, `handle_health` currently returns `projects: vec![]` and `agent_count: sessions.len()`. Exited handles remain until the 30-second session reaper removes them. This count is not an exact live-process count. `pu status` requires a project root resolved from `PU_PROJECT_ROOT` or cwd (`crates/pu-cli/src/commands/status.rs`, `commands/mod.rs`). Point Guard intentionally inherits neither a project root nor a builder identity (`apps/purepoint-mobile/docs/point-guard.md`). It consequently lacks a direct global inventory query.

Option A: CLI enumerates projects and issues one status request per project.
- Pro: Reuses project status and can produce a flattened JSON list.
- Con: Requires a discoverable project inventory anyway; repeats aggregation in clients, produces observations at different times, and leaves ownership routing to the caller.

Option B: Daemon owns a durable project registry and exposes a global status query plus an agent inventory query.
- Pro: CLI, desktop and Point Guard share query semantics, project discovery and agent ownership. Existing project manifests remain authoritative for durable project data, while live sessions supply process state.
- Con: Requires registry lifecycle rules, reconciliation after restart and explicit handling of unreadable projects. A persistent agent database would duplicate manifests; begin with a derived view and add an in-memory ownership index where useful.

Recommendation: Option B. Persist canonical project roots after successful initialization or validated use; restore the registry on startup without automatically spawning or resuming agents. Expose missing/unreadable projects rather than silently treating them as empty. Offer explicit project registration/removal, and keep app-open state separate from registration. Global means projects registered with the selected daemon, not every repository on disk; filesystem discovery can be an explicit import operation.

Proposed minimal CLI surface:
- `pu status --global [--json]`: compact totals and per-project summaries, without git diff scans, full prompts or logs.
- `pu projects list --json`: registered roots and availability.
- `pu agents list --global --state running --json`: flat live-agent records including project root, worktree id, id, name, type and observed state. Keep the existing `pu agent` namespace for saved agent definitions.
- `pu --project <root> <command>`: explicit routing that takes precedence over `PU_PROJECT_ROOT` and cwd. Preserve existing project-scoped defaults.
- Follow-up agent commands resolve ownership by id through the daemon when project scope is absent; return ambiguity errors rather than selecting an arbitrary match. An explicit project always constrains resolution.

The query model needs one shared definition of counts: running means a daemon-owned process with no observed exit; suspended agents count separately; terminal sessions are distinguished from AI agents by type. Existing persisted `AgentStatus::Running` alone is insufficient because suspended agents retain that status (`crates/pu-core/src/types/agent.rs`). Idle duration does not establish completion or a need for human input. Counts and list filtering must use the same derived state.

Machine-readable responses should include observation time, scope, completeness and per-project errors, with deterministic ordering and compact records. An incomplete inventory must never appear to be an exact global zero. Known live sessions should remain visible even if their project manifest is unavailable; live ownership can resolve those IDs without a complete durable inventory. Count/list queries should work outside a project, make one daemon request, and remain bounded under a slow or unreadable project. Avoid an arbitrary query language initially; explicit filters plus stable JSON support composition with tools such as jq.

First delivery should cover registry durability, a shared global query implementation, CLI routing, and bundled Point Guard instructions. Validate queries outside any project, two projects containing root and worktree agents, suspended and exited agents, restart recovery, duplicate path aliases, and partial project failures. Global subscriptions and broader worktree/schedule inventories can reuse the scope model later.

**Daemon startup sequence (from `pu-engine/src/main.rs`):** Parse args (looks for `--managed` flag and `--socket <path>`). Resolve socket path (`--socket` arg or `~/.pu/daemon.sock`). Init tracing. Take the `daemon.sock.lock` flock (exit 0 if another daemon holds it), then write the PID file (both modes) and raise the fd soft limit. Create `Engine`, bind `IpcServer` to socket (removes the stale socket file first, which is safe only because the lock is held). Run server until SIGTERM, SIGINT, or `Request::Shutdown`. On exit, stop agents gracefully, then remove the PID file and socket if the PID file still names this daemon.

**Shutdown handling:** SIGTERM and SIGINT are caught via `tokio::signal::unix`. `Request::Shutdown` from any client also triggers graceful shutdown via `Arc<Notify>`.

## Related

- [DAEMON-006] connects to [STORE-006] (spec indexing for retrieval) and [AGENT-005]/[AGENT-006] (context injection into agents) — these are different faces of the same context assembly problem.

# Daemon Engine

**Maturity: SEED**

## Context

PurePoint coordinates AI coding agents, worktrees, and project state across multiple projects simultaneously. A long-running daemon process is the natural architecture for this: it maintains persistent state, pushes real-time updates to clients, and manages long-lived agent sessions without requiring each CLI invocation to bootstrap from scratch. CLI and desktop app act as thin clients to this daemon. Getting the daemon's process model right is foundational to everything else.

## Open Questions

? [DAEMON-001] Should we use launchd or a PID-file for process supervision?
launchd is macOS-native and handles restart-on-crash, but requires a plist and is platform-specific. PID-file is simpler and cross-platform but means we handle our own crash recovery. The daemon needs to auto-start when CLI or app needs it.

? [DAEMON-002] How should the daemon handle multi-project state?
The daemon needs a global registry of all known projects, plus per-project state. Should this be a single global SQLite DB, separate per-project DBs, or a hybrid? How does the daemon discover projects — explicit registration, or scan for .pu/ directories?

? [DAEMON-003] What is the crash recovery model?
If the daemon crashes, agents may still be running in tmux. On restart, the daemon needs to reconcile its state with reality (running tmux sessions, existing worktrees). The current implementation includes recovery logic that re-scans tmux — should this be automatic on daemon startup?

? [DAEMON-004] What tokio runtime configuration is appropriate?
Multi-threaded vs current-thread runtime. How many worker threads. Whether to use tokio::spawn for agent monitoring or a dedicated watcher task. How to handle blocking operations (tmux commands, git operations) without blocking the async runtime.

? [DAEMON-005] What is the health and liveness model?
How do clients know the daemon is healthy? Health RPC, Unix socket probe, PID file + process check? How does the daemon signal degraded state (e.g., tmux server crashed but daemon is up)? What metrics should Health expose (uptime, project count, agent count, memory usage)?

? [DAEMON-006] How should the daemon classify incoming tasks for context assembly?
The daemon is the context assembler — every spawn/swarm/cron/send should automatically inject relevant specs and knowledge into agent prompts. How does the daemon decide what context is relevant? Options: keyword matching against spec file names/tags, LLM classification of the task description, explicit tags in the spawn command (e.g., `pu spawn --context tdd,architecture`), or template-defined context maps where swarm/prompt templates declare their context needs.

## Design Directions

- macOS (primary) and Linux (future)
- Auto-start when CLI or app needs it and it's not running
- Graceful shutdown (SIGTERM) with resource cleanup
- No root/sudo requirement
- Support for multiple projects simultaneously

## Related

- [DAEMON-006] connects to [STORE-006] (spec indexing for retrieval) and [AGENT-005]/[AGENT-006] (context injection into agents) — these are different faces of the same context assembly problem.

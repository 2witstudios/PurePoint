# Daemon Engine

**Maturity: SEED**

## Context

PurePoint coordinates AI coding agents, worktrees, and project state across multiple projects simultaneously. A long-running daemon process is the natural architecture for this: it maintains persistent state, pushes real-time updates to clients, and manages long-lived agent sessions without requiring each CLI invocation to bootstrap from scratch. CLI and desktop app act as thin clients to this daemon. Getting the daemon's process model right is foundational to everything else.

## Open Questions

? [DAEMON-001] How should the daemon process be supervised?
Options include OS-native supervision (launchd on macOS, systemd on Linux), a PID-file with self-managed recovery, or a wrapper process. The daemon needs to auto-start when CLI or app needs it. Trade-offs: platform-specificity vs simplicity vs crash recovery.

? [DAEMON-002] How should the daemon handle multi-project state?
The daemon needs a global registry of all known projects, plus per-project state. Should this be a single global store, separate per-project stores, or a hybrid? How does the daemon discover projects — explicit registration, or scan for `.pu/` directories?

? [DAEMON-003] What is the crash recovery model?
If the daemon crashes, agents may still be running. On restart, the daemon needs to reconcile its state with reality. Should this be automatic on daemon startup?

? [DAEMON-004] What runtime model is appropriate?
Async vs threaded. How to handle blocking operations (process management, git operations) without blocking the event loop. What concurrency model serves the daemon's needs.

? [DAEMON-005] What is the health and liveness model?
How do clients know the daemon is healthy? How does the daemon signal degraded state? What metrics should be exposed?

? [DAEMON-006] How should the daemon classify incoming tasks for context assembly?
The daemon is the context assembler — every task should automatically inject relevant specs and knowledge into agent prompts. How does the daemon decide what context is relevant? Options: keyword matching, task classification, explicit tags in spawn commands, or template-defined context maps.

## Design Directions

- Primary platform: macOS. Future: Linux.
- Auto-start when CLI or app needs it and it's not running
- Graceful shutdown with resource cleanup
- No root/sudo requirement
- Support for multiple projects simultaneously

## Related

- [DAEMON-006] connects to [STORE-006] (spec indexing for retrieval) and [AGENT-005]/[AGENT-006] (context injection into agents) — these are different faces of the same context assembly problem.

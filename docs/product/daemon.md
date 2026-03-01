# Daemon

**Maturity: SEED** | ID Prefix: DMN | Dependencies: `architecture/daemon-engine.md`, `architecture/ipc-api.md`

## Purpose

The long-running background process that owns all state and operations. Manages agent processes, worktrees, scheduling, and serves the API. Single source of truth for the entire system.

## Conceptual Model

```
Daemon
  API Server (clients connect to request operations)
  Project Manager (per-project state)
  Agent Monitor (watches agent processes)
  Worktree Tracker
  Scheduler (timed/recurring tasks)
  Output Capturer (streams agent output to clients)
  Event Bus (internal pub/sub for state changes)
```

## Open Questions

? [DMN-001] Should the daemon support multiple concurrent projects, or one daemon instance per project?

? [DMN-002] How should the daemon handle version mismatches between CLI and daemon (e.g., after an update)?

# Daemon

**Maturity: SEED** | ID Prefix: DMN

## Purpose

The long-running Rust background process that owns all state and operations. Manages tmux sessions, git worktrees, agent processes, scheduling, and the gRPC server. Single source of truth for the entire system.

## Conceptual Model

```
Daemon (tokio async runtime)
  gRPC Server (tonic)
    Unix socket listener (local clients)
    TCP listener (remote clients, future)
  Project Manager
    Per-project state (SQLite DB)
    Agent monitor (watches tmux panes)
    Worktree tracker
  Scheduler (cron)
  Output Capturer (streams pane output)
  Event Bus (internal pub/sub for state changes)
```

## Interfaces

See Architecture/IPC & API for gRPC service definitions.

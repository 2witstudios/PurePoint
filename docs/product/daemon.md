# Daemon

**Maturity: EXPLORING** | ID Prefix: DMN | Dependencies: `architecture/daemon-engine.md`, `architecture/ipc-api.md`

## Purpose

The long-running background process that owns all state and operations. Manages agent processes, worktrees, scheduling, and serves the API. Single source of truth for the entire system.

## Conceptual Model

```
Daemon (pu-engine binary)
  Engine (core state: projects, agents, worktrees, manifest I/O)
  IPC Server (Unix socket listener, connection pool, request routing)
  PTY Manager (native PTY host: fork/setsid/execvp, master fd ownership)
  Agent Monitor (effective_status: exit code + prompt detection + idle timeout)
  Output Buffers (1MB circular buffer per agent)
  Git Integration (worktree create/remove via git commands)
  Daemon Lifecycle (PID file, socket cleanup, signal handling, shutdown)
```

## Research Notes

**DMN-001: Single daemon, per-project state keyed by project root.** The daemon is a single process serving all projects. Each request includes a `project_root` parameter to scope operations. `Engine` maintains per-project state internally. `Request::Init { project_root }` registers a project; subsequent `Spawn`/`Status`/`Kill` requests reference it. `Response::HealthReport` includes a `projects: Vec<String>` listing all registered project roots and `agent_count: usize` across all projects.

**Daemon binary:** `pu-engine` (separate from `pu-cli`). Standalone mode writes PID file and cleans up on exit; managed mode (`--managed`) skips PID file for when the CLI controls the lifecycle. Socket path configurable via `--socket <path>`, defaults to `~/.pu/daemon.sock`.

## Open Questions

? [DMN-001] Should the daemon support multiple concurrent projects, or one daemon instance per project?
(Research note above shows current implementation is single-daemon multi-project, but the tradeoffs haven't been fully evaluated.)

? [DMN-002] How should the daemon handle version mismatches between CLI and daemon (e.g., after an update)?
(Protocol version is in HealthReport but no mismatch handling is implemented yet.)

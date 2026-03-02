# Agent Lifecycle

**Maturity: CONVERGING** | ID Prefix: AL | Dependencies: `architecture/agent-execution.md`

## Purpose

The complete lifecycle of an AI agent from spawn to completion: creation, monitoring, status detection, output capture, restart, and cleanup.

## Conceptual Model

```
States: Spawning → Running → Idle → Completed | Failed | Killed | Lost
  Spawning: PTY allocated, process forking
  Running: agent process active, producing output
  Idle: shell prompt detected OR 30s output inactivity
  Completed: exit code 0
  Failed: nonzero exit code (including 128+signal)
  Killed: terminated via pu kill (SIGTERM → SIGKILL)
  Lost: process disappeared unexpectedly
```

Terminal states (no further transitions): Completed, Failed, Killed, Lost.
Non-terminal states: Spawning, Running, Idle.

## Decisions

! [AL-001] Shell prompt pattern matching (`$ `, `% `, `# `, `> `) plus 30-second output inactivity timeout — `effective_status()` is a pure function (no polling loop) called on-demand when computing status for requests. Checks last 256 bytes of output buffer for prompt patterns (UTF-8 lossy, trailing whitespace stripped). If prompt detected OR `idle_seconds() > 30`, status is Idle. If exit code present: 0 = Completed, nonzero = Failed. Otherwise Running. Implemented in `pu-engine/src/agent_monitor.rs`.

## Research Notes

**Status enum (`pu-core/src/types.rs`):** `AgentStatus` variants: `Spawning`, `Running`, `Idle`, `Completed`, `Failed`, `Killed`, `Lost`. Serde-serialized as camelCase. `is_terminal()` returns true for Completed/Failed/Killed/Lost.

**Effective status is computed live, not stored.** The manifest stores the last-known status, but `effective_status()` computes the real status from PTY state (exit code from `waitpid` watch channel, output buffer idle time, prompt detection). This means status reported via `pu status` reflects the actual current state, not a stale manifest snapshot.

**Status from daemon (macOS app):** The desktop app queries status from the daemon via IPC (`status` request → `status_report` response). A manifest file watcher triggers re-queries on file changes (300ms debounce), but the data source is the daemon, not the manifest file directly.

**Terminal connection (macOS app):** The daemon streams output via the IPC `Attach`/`Output` protocol. The desktop app connects via `DaemonWorkspaceService`, which issues an `Attach` request for the agent and receives `Output` messages with live PTY data. Multiple viewers can attach to the same agent simultaneously.

## Open Questions

? [AL-002] Should agents support pause/resume, or only the full lifecycle (spawn → running → exited)?

**App quit behavior:** When the macOS app quits, it sends `Request::Shutdown` to the daemon. The daemon kills all agent processes and exits. Agent state (command, worktree, last status) is persisted in the manifest. On next app launch, agents can be re-spawned from the saved state. Standalone CLI mode does not auto-shutdown — agents persist until explicitly killed or the daemon is stopped.

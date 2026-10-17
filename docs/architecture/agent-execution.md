# Agent Execution

**Maturity: CONVERGING**

## Context

PurePoint manages AI coding agents that run in isolated environments. Each agent needs its own workspace (git worktree), a process host, lifecycle management (spawn, monitor, detect completion), and output capture for streaming to clients. The execution model determines how agents interact with the system and how clients observe them.

## Decisions

! [AGENT-001] Two coexisting execution paths for process hosting:

**Daemon (pu-engine):** Native PTY host via `fork` + `setsid` + `execvp`. The daemon owns the PTY master fd directly for programmatic control. Uses `nix::pty::openpty` for PTY allocation, sets window size via `TIOCSWINSZ` before fork. Child process: `setsid()` → `TIOCSCTTY` → `dup2` slave to 0/1/2 → close fds 3..1024 → `chdir` → `setenv` → `execvp`. All `CString` allocations happen pre-fork to avoid allocator lock contention in the async runtime. `posix_spawn` rejected because it can't do `setsid`, `TIOCSCTTY`, or close arbitrary fds. Implemented in `pu-engine/src/pty_manager.rs`. Clients (desktop app, CLI) connect via the IPC `Attach`/`Output` protocol to stream terminal output without affecting agent processes.

! [AGENT-002] Native PTY master fd read loop into 1MB circular `OutputBuffer` — reader task runs in `spawn_blocking`, reading 4096-byte chunks from the raw master fd into a `VecDeque<u8>`-based buffer with `RwLock` for concurrent read access. Overflow drains oldest bytes from the front. Thread-safe: multiple concurrent readers (status checks, log requests), exclusive writer (PTY reader task). Implemented in `pu-engine/src/output_buffer.rs` (buffer), `pu-engine/src/pty_manager.rs` (reader task spawn).

! [AGENT-004] `waitpid(WUNTRACED)` in `spawn_blocking` for exit detection — exit codes are retained as diagnostics (signal death = 128 + signal number); any observed process exit maps to Broken. Kill sequence: SIGTERM → poll every 200ms up to grace period → SIGKILL → wait 100ms. Exit status delivered via `tokio::sync::watch` channel. Process liveness is `Running` without an observed exit and `Broken` for any exit; `effective_status(exit_code)` is a pure function called on demand. Exit code and terminal silence do not prove a successful turn. Optional passive launch-bound signal snapshots describe activity, pending attention and outcomes independently (see [Agent Lifecycle, AL-001](../product/agent-lifecycle.md#decisions)); observation cannot change PTY/session ownership or input/resume/shutdown semantics. Implemented in `pu-engine/src/pty_manager.rs` (waitpid, kill), `pu-engine/src/agent_monitor.rs` (process status), and `pu-core/src/signals.rs` (pure signal contract/reducer).

## Open Questions

? [AGENT-003] Should there be a remote execution model?
Should PurePoint support spawning agents on remote machines? Not needed initially, but architecture decisions now could make it easier or harder later.

? [AGENT-005] How should context be injected into agent prompts?
The daemon assembles context (specs, memory, project state) for each agent task. How does this context reach the agent? Options: prepend to prompt, generate a system message, write a config file in the worktree, environment variables, stdin, or a combination.

? [AGENT-006] Should context assembly be configurable per-agent-type?
Different agent types may need different context formats and injection methods. Should the daemon have pluggable context formatters, or should all agents receive context the same way?

## Design Directions

- Support for multiple agent types (configurable via `.pu/config.yaml` — `AgentConfig` with command, prompt_flag, interactive flag)
- Agent output capture for streaming and log retrieval (1MB circular buffer per agent)
- Process exit detection via waitpid; turn outcome requires authoritative native observation
- Input delivery to running agents (write to PTY master fd via `spawn_blocking`)
- PTY resize support (TIOCSWINSZ ioctl on master fd)
- Agent crash handling without corrupting daemon state (watch channel for exit notification)

## Related

- [AGENT-005]/[AGENT-006] connect to [DAEMON-006] (context classification) and [STORE-006] (spec indexing) — classification determines what context is relevant, storage retrieves it, agent execution injects it.

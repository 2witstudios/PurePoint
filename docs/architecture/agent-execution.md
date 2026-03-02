# Agent Execution

**Maturity: CONVERGING**

## Context

PurePoint manages AI coding agents that run in isolated environments. Each agent needs its own workspace (git worktree), a process host, lifecycle management (spawn, monitor, detect completion), and output capture for streaming to clients. The execution model determines how agents interact with the system and how clients observe them.

## Decisions

! [AGENT-001] Two coexisting execution paths for process hosting:

**Daemon (pu-engine):** Native PTY host via `fork` + `setsid` + `execvp`. The daemon owns the PTY master fd directly for programmatic control — no tmux dependency. Uses `nix::pty::openpty` for PTY allocation, sets window size via `TIOCSWINSZ` before fork. Child process: `setsid()` → `TIOCSCTTY` → `dup2` slave to 0/1/2 → close fds 3..1024 → `chdir` → `setenv` → `execvp`. All `CString` allocations happen pre-fork to avoid allocator lock contention in the async runtime. `posix_spawn` rejected because it can't do `setsid`, `TIOCSCTTY`, or close arbitrary fds. Implemented in `pu-engine/src/pty_manager.rs`.

**macOS app:** tmux as process host (original AGENT-001 decision, still valid for human-facing terminal). Grouped sessions for independent viewer tracking — each desktop app terminal connects via `SwiftTerm LocalProcessTerminalView → forkpty → /bin/zsh -c "tmux new-session -t ..." → grouped session attach`. Multiple viewers can attach to the same agent session. Proven at 30 agents with low CPU/memory in ppg-cli.

**Research note (ppg-cli architecture):** The desktop app creates a `LocalProcessTerminalView` (SwiftTerm) which forks a PTY running `/bin/zsh`. The shell sources profile scripts for PATH resolution (critical on M-series Macs where `/opt/homebrew/bin` isn't in GUI app PATH), then execs `tmux new-session -t {session} -s {viewSession}` with `destroy-unattached on`. The `-t` flag creates a grouped session sharing the same windows but with independent current-window tracking. A random suffix on the view session name prevents collisions during fast re-selection after LRU eviction.

! [AGENT-002] Native PTY master fd read loop into 1MB circular `OutputBuffer` — reader task runs in `spawn_blocking`, reading 4096-byte chunks from the raw master fd into a `VecDeque<u8>`-based buffer with `RwLock` for concurrent read access. Overflow drains oldest bytes from the front. Thread-safe: multiple concurrent readers (status checks, log requests), exclusive writer (PTY reader task). Implemented in `pu-engine/src/output_buffer.rs` (buffer), `pu-engine/src/pty_manager.rs` (reader task spawn).

! [AGENT-004] `waitpid(WUNTRACED)` in `spawn_blocking` for exit detection — exit code 0 = completed, nonzero = failed, signal death = 128 + signal number. Kill sequence: SIGTERM → poll every 200ms up to grace period → SIGKILL → wait 100ms. Exit status delivered via `tokio::sync::watch` channel. Idle detection via shell prompt pattern matching (`$ `, `% `, `# `, `> ` at end of last 256 bytes) OR 30-second output inactivity timeout. `effective_status()` is a pure function called on-demand, not a polling loop. Implemented in `pu-engine/src/pty_manager.rs` (waitpid, kill), `pu-engine/src/agent_monitor.rs` (effective_status).

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
- Agent completion detection (exit code via waitpid, idle detection via prompt/timeout)
- Input delivery to running agents (write to PTY master fd via `spawn_blocking`)
- PTY resize support (TIOCSWINSZ ioctl on master fd)
- Agent crash handling without corrupting daemon state (watch channel for exit notification)

## Related

- [AGENT-005]/[AGENT-006] connect to [DAEMON-006] (context classification) and [STORE-006] (spec indexing) — classification determines what context is relevant, storage retrieves it, agent execution injects it.

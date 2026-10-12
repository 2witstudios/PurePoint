# Agent Execution

**Maturity: CONVERGING**

## Context

PurePoint manages AI coding agents that run in isolated environments. Each agent needs its own workspace (git worktree), a process host, lifecycle management (spawn, monitor, detect completion), and output capture for streaming to clients. The execution model determines how agents interact with the system and how clients observe them.

## Decisions

! [AGENT-001] Two coexisting execution paths for process hosting:

**Daemon (pu-engine):** Native PTY host via `fork` + `setsid` + `execvp`. The daemon owns the PTY master fd directly for programmatic control. Uses `nix::pty::openpty` for PTY allocation, sets window size via `TIOCSWINSZ` before fork. Child process: `setsid()` → `TIOCSCTTY` → `dup2` slave to 0/1/2 → close fds 3..1024 → `chdir` → `setenv` → `execvp`. All `CString` allocations happen pre-fork to avoid allocator lock contention in the async runtime. `posix_spawn` rejected because it can't do `setsid`, `TIOCSCTTY`, or close arbitrary fds. Implemented in `pu-engine/src/pty_manager.rs`. Clients (desktop app, CLI) connect via the IPC `Attach`/`Output` protocol to stream terminal output without affecting agent processes.

! [AGENT-002] Native PTY master fd read loop into 1MB circular `OutputBuffer` — reader task runs in `spawn_blocking`, reading 4096-byte chunks from the raw master fd into a `VecDeque<u8>`-based buffer with `RwLock` for concurrent read access. Overflow drains oldest bytes from the front. Thread-safe: multiple concurrent readers (status checks, log requests), exclusive writer (PTY reader task). Implemented in `pu-engine/src/output_buffer.rs` (buffer), `pu-engine/src/pty_manager.rs` (reader task spawn).

! [AGENT-004] `waitpid(WUNTRACED)` in `spawn_blocking` for exit detection — exit code 0 = completed, nonzero = failed, signal death = 128 + signal number. Kill sequence: SIGTERM → poll every 200ms up to grace period → SIGKILL → wait 100ms. Exit status delivered via `tokio::sync::watch` channel. Idle detection via shell prompt pattern matching (`$ `, `% `, `# `, `> ` at end of last 256 bytes) OR 30-second output inactivity timeout. `effective_status()` is a pure function called on-demand, not a polling loop. Implemented in `pu-engine/src/pty_manager.rs` (waitpid, kill), `pu-engine/src/agent_monitor.rs` (effective_status).

## Open Questions

? [AGENT-003] Should there be a remote execution model?
The owner confirmed a local-network fleet where one host Point Guard runs agents on any enrolled machine (2026-10-11). Product intent is resolved; the v1 topology is hub-and-spoke. Revision 2 of the proposal under [IPC-004] in `ipc-api.md` defines proposed per-agent controller epochs, serialized local/remote mutation handling, non-replayable PTY input, atomic admission and durable agent/run identities. The security scope and platform mechanism for daemon-subordinate process lifetime must be resolved before remote execution implementation. Architectural direction is supported by the owner-supplied review; implementation approval/readiness is not established.

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

## Research Notes

### [AGENT-003] LAN execution review: control ownership and lifecycle

The owner-supplied architecture review identifies four authorities: daemon identity, receiver authorization, worker execution ownership and designated per-agent control. The worker remains authoritative when the host disappears. Project exports constrain routing but do not sandbox agents executing as the daemon's OS user; a trusted personal fleet is the proposed v1 scope, pending owner confirmation.

Current implementation evidence: `engine/spawn.rs` generates the agent ID before launch but persists the agent manifest after `pty_host.spawn` and in-memory session registration. Failure returned by persistence triggers cleanup, but abrupt death needs a durable pre-launch operation/run record. `engine/pty_operations.rs` serializes `deliver_text`; raw input and attach-stream writes in `ipc_server/streams.rs` bypass that delivery lock. PTY handles/output readers/wait tasks live inside the daemon in `pty_manager.rs`. These paths do not establish independent crash-surviving supervision or guaranteed abrupt-death cleanup.

Proposed foundation: one per-agent mutation boundary across local clients, bridge clients, raw attach and automation; durable controller epoch and run identity fence stale commands; one writer sequences PTY input and reports accepted/written/partial/unknown without automatic uncertain replay. Local administrative takeover is explicit. Admission covers local and remote spawn/resume before launch, reserving capacity and durable intent together. Revocation removes control and observation but leaves worker-owned runs for local disposition by default.

The proposed v1 lifecycle ends managed runs when their owning worker daemon dies, while host disconnect leaves them running. A supported-platform process-tree supervision mechanism and failure-injection evidence are prerequisites, not current behavior. Recovery must prove termination before freeing capacity or explicitly resuming into a new run; an old PID alone is insufficient. See [DAEMON-003] in `daemon-engine.md` and the full contracts/vertical delivery slices under [IPC-004]. Durable terminal spooling, automatic migration and independently surviving PTYs are deferred.

## Related

- [AGENT-003] connects to [IPC-004] (secure daemon mesh). Proposed model: host routes machine-qualified operations; receiving workers own PTYs, worktrees, local runtime/provider credentials and authoritative agent state. Disconnect does not stop remote agents; uncertain spawns require reconciliation rather than automatic replay or replacement on another machine. The host aggregates observations without replicating worker manifests. Source commits must be verified against explicitly bound worker repositories; repository transfer is a separate concern. These are design proposals, not implemented behavior.

- [AGENT-005]/[AGENT-006] connect to [DAEMON-006] (context classification) and [STORE-006] (spec indexing) — classification determines what context is relevant, storage retrieves it, agent execution injects it.

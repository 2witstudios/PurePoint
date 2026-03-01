# Agent Execution

**Maturity: SEED**

## Context

PurePoint manages AI coding agents that run in isolated environments. Each agent needs its own workspace (git worktree), a process host, lifecycle management (spawn, monitor, detect completion), and output capture for streaming to clients. The execution model determines how agents interact with the system and how clients observe them.

## Decisions

! [AGENT-001] tmux as process host. Proven at 30 agents with low CPU/memory in ppg-cli. Grouped sessions for independent viewer tracking — each desktop app terminal connects via `SwiftTerm LocalProcessTerminalView → forkpty → /bin/zsh -c "tmux new-session -t ..." → grouped session attach`. Multiple viewers can attach to the same agent session without interfering with each other's window focus.

**Research note (ppg-cli architecture):** The desktop app creates a `LocalProcessTerminalView` (SwiftTerm) which forks a PTY running `/bin/zsh`. The shell sources profile scripts for PATH resolution (critical on M-series Macs where `/opt/homebrew/bin` isn't in GUI app PATH), then execs `tmux new-session -t {session} -s {viewSession}` with `destroy-unattached on`. The `-t` flag creates a grouped session sharing the same windows but with independent current-window tracking. A random suffix on the view session name prevents collisions during fast re-selection after LRU eviction.

## Open Questions

? [AGENT-002] How should real-time output capture work?
For streaming to clients, the daemon needs continuous output. Options depend on the hosting model — polling, pipe-based capture, PTY pairs, or process substitution. Each has latency and completeness trade-offs.

? [AGENT-003] Should there be a remote execution model?
Should PurePoint support spawning agents on remote machines? Not needed initially, but architecture decisions now could make it easier or harder later.

? [AGENT-004] How should crash vs clean exit be detected?
Detecting agent state reliably. Options: monitor child process PID directly, use process host features, or wrap agents in a supervisor.

? [AGENT-005] How should context be injected into agent prompts?
The daemon assembles context (specs, memory, project state) for each agent task. How does this context reach the agent? Options: prepend to prompt, generate a system message, write a config file in the worktree, environment variables, stdin, or a combination.

? [AGENT-006] Should context assembly be configurable per-agent-type?
Different agent types may need different context formats and injection methods. Should the daemon have pluggable context formatters, or should all agents receive context the same way?

## Design Directions

- Support for multiple agent types
- Agent output capture for streaming and log retrieval
- Agent completion detection (exit code, state transitions)
- Input delivery to running agents
- Agent crash handling without corrupting daemon state

## Related

- [AGENT-005]/[AGENT-006] connect to [DAEMON-006] (context classification) and [STORE-006] (spec indexing) — classification determines what context is relevant, storage retrieves it, agent execution injects it.

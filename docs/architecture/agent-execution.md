# Agent Execution

**Maturity: SEED**

## Context

PurePoint manages AI coding agents (Claude, Codex, OpenCode, terminal) that run in isolated environments. Each agent needs its own workspace (git worktree), a process host (tmux pane or PTY), lifecycle management (spawn, monitor, detect completion), and output capture for streaming to clients. The execution model determines how agents interact with the system and how clients observe them.

## Open Questions

? [AGENT-001] Should agents run in tmux managed by the daemon, or in daemon-managed PTYs?
tmux provides persistence (agents survive daemon crash), multiplexing, and user attachment. But it adds a dependency and complexity. Daemon-managed PTYs give full control but lose persistence. Hybrid: use tmux for persistence but capture output via daemon PTY proxy?

? [AGENT-002] How should real-time output capture work?
For streaming to the dashboard, the daemon needs continuous output. Options: poll `capture-pane` on interval, use tmux pipe-pane to a named pipe, create a PTY pair where daemon is the master, or use process substitution. Each has latency/completeness tradeoffs.

? [AGENT-003] Should there be a cloud/remote execution model?
One approach is local-only execution. Should PurePoint support spawning agents on remote machines or cloud instances? This would require SSH tunneling or a remote daemon. Not needed for v1 but architecture decisions now could make it easier or harder later.

? [AGENT-004] How should crash vs clean exit be detected?
Pattern-matching pane output is fragile — it depends on shell prompt format and agent-specific output patterns. Better options: monitor the child process PID directly, use tmux's `remain-on-exit` + `pane_dead` format, or wrap agents in a thin supervisor script that writes exit status to a known location.

? [AGENT-005] How should context be injected into agent prompts?
The daemon assembles context (specs, memory, project state) for each agent task. How does this context reach the agent? Options: prepend to the user prompt text, generate a separate system message, write a temporary CLAUDE.md in the worktree, set environment variables, pipe via stdin, or a combination. Different agent types may need different injection methods.

? [AGENT-006] Should context assembly be configurable per-agent-type?
Claude Code reads CLAUDE.md, Codex reads different formats, terminal agents need different context than coding agents. Should the daemon have pluggable context formatters per agent type? Or should all agents receive context the same way and handle it themselves?

## Design Directions

- Support for multiple agent types: claude, codex, opencode, terminal
- Agent output capture for dashboard streaming and log retrieval
- Agent completion detection (exit code, running/idle/exited/gone states)
- Input delivery to running agents (for prompts, follow-up commands)
- Agent crash handling without corrupting daemon state

## Related

- [AGENT-005]/[AGENT-006] connect to [DAEMON-006] (context classification) and [STORE-006] (spec indexing) — classification determines what context is relevant, storage retrieves it, agent execution injects it.

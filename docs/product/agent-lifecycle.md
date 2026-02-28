# Agent Lifecycle

**Maturity: SEED** | ID Prefix: AL

## Purpose

The complete lifecycle of an AI agent from spawn to completion: creation, monitoring, status detection, output capture, restart, and cleanup.

## Conceptual Model

```
States: created → running → idle → exited | gone
  created: worktree ready, tmux pane opened, command not yet sent
  running: agent process active, producing output
  idle: agent waiting for input (shell prompt detected)
  exited: agent process terminated (has exit code)
  gone: tmux pane disappeared (crash or manual close)
```

## Interfaces

```
AgentEntry { id, name, agentType, status, tmuxTarget, prompt, exitCode, sessionId, startedAt, updatedAt }
AgentStatus = running | idle | exited | gone
AgentType = claude | codex | opencode | terminal
```

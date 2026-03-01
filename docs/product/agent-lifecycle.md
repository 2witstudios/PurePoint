# Agent Lifecycle

**Maturity: SEED** | ID Prefix: AL | Dependencies: `architecture/agent-execution.md`

## Purpose

The complete lifecycle of an AI agent from spawn to completion: creation, monitoring, status detection, output capture, restart, and cleanup.

## Conceptual Model

```
States: created → running → idle → exited | gone
  created: worktree ready, process host opened, command not yet sent
  running: agent process active, producing output
  idle: agent waiting for input
  exited: agent process terminated
  gone: process host disappeared (crash or manual close)
```

## Research Notes

**Status from manifest:** Agent status is read from the `status` field in `.pu/manifest.json` agent entries. Values: spawning, running, waiting, completed, failed, killed, lost. The desktop app polls status via manifest file watching (DispatchSource on file changes with 300ms debounce).

**Terminal connection:** Each agent entry contains a `tmuxTarget` field (format: `session:window` or `session:window.pane`) used to create grouped tmux sessions for terminal display.

## Open Questions

? [AL-001] How should idle detection work — shell prompt pattern matching, output timeout, or process state inspection?

? [AL-002] Should agents support pause/resume, or only the full lifecycle (spawn → running → exited)?

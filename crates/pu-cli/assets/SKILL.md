---
name: pu
description: Use when user asks to spawn agents, check workspace status, manage parallel coding tasks, schedule future agents, or when you need to run work in isolated worktrees. PurePoint orchestrates parallel AI coding agents.
user-invocable: true
---

# PurePoint (`pu`)

Spawn parallel AI coding agents in isolated git worktrees. Each agent gets its own branch, terminal, and working directory.

## Status Model

Agents have three observable states:

| Status | Meaning |
|---|---|
| `streaming` | Output flowing — agent is actively working |
| `waiting` | Process alive but idle — likely awaiting a prompt or input |
| `broken` | Process exited or gone — check exit_code for details |

## Agent Lifecycle Rules

**When operating as an agent, follow these rules:**

1. **Never clean up existing agents or worktrees unless the user explicitly asks.** When given a task, spawn ADDITIONAL agents/worktrees. Do not remove existing ones first.
2. **Never run `pu kill` preemptively.** Only kill agents the user specifically asks you to stop.
3. **Prefer targeted kills.** Use `pu kill --agent <id>` or `pu kill --worktree <id>` for specific cleanup. Avoid `pu kill --all`.
4. **`pu kill --all` preserves root agents** (point guards/conductors). Use `--include-root` only if the user explicitly requests it.
5. **Check status before spawning.** Run `pu status --json` to see what exists. Add alongside it — don't replace it.
6. **You cannot kill yourself.** The CLI refuses to kill the calling agent's own process.

## Commands

### Spawn agents
```bash
pu spawn "fix the auth bug" --name fix-auth              # worktree + agent
pu spawn "refactor tests" --name refactor --agent codex   # use codex
pu spawn "run the dev server" --root                      # root agent (no worktree)
pu spawn --root --agent terminal                          # plain terminal
pu spawn --template code-review --var BRANCH=main         # from saved prompt
pu spawn --file path/to/prompt.md --name task1            # from file
pu spawn "prompt" --name fix-auth --json                  # machine output
```

### Check status
```bash
pu status                          # all agents and worktrees
pu status --agent ag-xxx           # single agent
pu status --json                   # machine-readable
```

### Read agent output
```bash
pu logs <agent_id>                 # last 500 bytes of PTY output
pu logs <agent_id> --tail 2000     # more context
pu logs <agent_id> --json          # { "agent_id": "...", "data": "..." }
```

### Send input to agents
```bash
pu send <agent_id> "fix the auth bug too"   # sends text + Enter
pu send <agent_id> "text" --no-enter        # sends text without Enter
pu send <agent_id> --keys "C-c"             # send Ctrl+C
```

### Kill agents
```bash
pu kill --agent <agent_id>                    # kill one agent
pu kill --worktree <wt_id>                    # kill all agents in worktree
pu kill --all                                 # kill worktree agents (preserves root/point guard agents)
pu kill --all --include-root                  # kill everything including root agents
pu kill --agent <agent_id> --json             # machine output
```

### Clean up worktrees
```bash
pu clean --worktree <wt_id>              # remove worktree, kill its agents, delete branch
pu clean --all                           # remove all worktrees
pu clean --all --json                    # machine output
```

### Schedule agents
```bash
pu schedule list                   # list all schedules
pu schedule list --json            # machine-readable
pu schedule show <name>            # show schedule details
pu schedule show <name> --json     # machine-readable

# Worktree spawn (default) — needs --name
pu schedule create overnight-build \
  --start-at "2026-03-07T22:30:00" \
  --name overnight-build \
  --trigger inline-prompt \
  --trigger-prompt "build a feature"   # spawns in worktree pu/overnight-build

# Root spawn — for read-only/cross-project tasks
pu schedule create morning-status \
  --start-at "2026-03-07T08:00:00" \
  --root \
  --trigger inline-prompt \
  --trigger-prompt "scan commits"      # spawns in project root

# Recurring with agent def
pu schedule create nightly-review \
  --start-at "2026-03-07T03:00:00" \
  --recurrence daily \
  --root \
  --trigger agent-def \
  --trigger-name security-review       # recurring root agent from saved def

pu schedule enable <name>          # enable a disabled schedule
pu schedule disable <name>         # disable without deleting
pu schedule delete <name>          # remove a schedule
```

**Recurrence options**: `none` (one-shot), `hourly`, `daily`, `weekdays`, `weekly`, `monthly`.

**Trigger types**: `inline-prompt` (with `--trigger-prompt`), `agent-def` (with `--trigger-name`), `swarm-def` (with `--trigger-name`).

**Spawn mode**: By default, scheduled agents spawn into a worktree (requires `--name`). Use `--root` to spawn in the project root instead (for read-only or cross-project tasks).

**Scope**: `--scope local` (default, project-level) or `--scope global`.

### Other
```bash
pu health                          # daemon status
pu health --json                   # machine-readable
pu prompt list                     # list saved prompt templates
pu prompt list --json              # machine-readable
pu init                            # initialize workspace
pu attach <agent_id>               # interactive terminal attach
```

## Saved Prompt Templates

Templates live in `.pu/templates/*.md` (project) and `~/.pu/templates/*.md` (global).

Format:
```markdown
---
name: code-review
description: Review code for quality
agent: codex
---
Review the code on branch {{BRANCH}} for quality and security.
{{CONTEXT}}
```

Use with: `pu spawn --template code-review --var BRANCH=main`

## Reading Conversation History

Claude agents have a `session_id` visible in `pu status --json`. Session transcripts are stored at `~/.claude/projects/{project-hash}/*.jsonl`. You can grep for the session ID to find the right transcript, or use `pu logs <agent_id>` to read the PTY output directly.

## `--json` Flag

All commands support `--json` for machine-readable output. Always use this when parsing output programmatically.

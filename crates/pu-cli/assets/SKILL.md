---
name: pu
description: Use when user asks to spawn agents, check workspace status, manage parallel coding tasks, or when you need to run work in isolated worktrees. PurePoint orchestrates parallel AI coding agents.
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
pu kill --agent <agent_id>         # kill one agent
pu kill --worktree <wt_id>         # kill all in worktree
pu kill --all                      # kill everything
pu kill --agent <agent_id> --json  # machine output
```

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

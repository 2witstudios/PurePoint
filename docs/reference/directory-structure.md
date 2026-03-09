# Directory Structure

PurePoint uses two directory trees: a project-level `.pu/` directory and a global `~/.pu/` directory.

## Project directory (`.pu/`)

Created by `pu init` in your project root.

```
.pu/
├── manifest.json          # Workspace state (source of truth)
├── config.yaml            # Project configuration
├── agent-context.md       # Context for non-Claude agents (codex, opencode)
├── grid-layout.json       # Pane grid layout persistence (macOS app)
├── worktrees/             # Git worktree directories
│   └── wt-{id}/          # Individual worktree checkout
├── prompts/               # Per-agent prompt files
│   └── {agent-id}.md     # Prompt used for a specific agent
├── templates/             # Saved prompt templates (local scope)
│   └── {name}.md         # Markdown with YAML frontmatter
├── agents/                # Saved agent definitions (local scope)
│   └── {name}.yaml       # Agent def YAML
├── swarms/                # Saved swarm definitions (local scope)
│   └── {name}.yaml       # Swarm def YAML
├── schedules/             # Saved schedule definitions (local scope)
│   └── {name}.yaml       # Schedule def YAML
└── triggers/              # Saved trigger definitions (local scope)
    └── {name}.yaml       # Trigger def YAML
```

### Key files

| File | Format | Managed by | Editable |
|---|---|---|---|
| `manifest.json` | JSON | Daemon | No (daemon-managed) |
| `config.yaml` | YAML | User / `pu init` | Yes |
| `agent-context.md` | Markdown | `pu init` | Yes |
| `grid-layout.json` | JSON | macOS app | No (app-managed) |

### Definition directories

Templates, agent defs, swarm defs, schedules, and triggers are all user-editable. You can create them via CLI commands or by writing YAML/Markdown files directly. See the corresponding docs for file formats:

- Templates: [Templates and Definitions](../guide/workflows/templates-and-definitions.md)
- Config: [Configuration](../guide/configuration.md)
- Schedules/Triggers: [Scheduling and Triggers](../guide/workflows/scheduling-and-triggers.md)

## Global directory (`~/.pu/`)

Created automatically. Holds daemon runtime files and global-scope definitions.

```
~/.pu/
├── bin/
│   └── pu                 # CLI binary (installed by macOS app)
├── daemon.pid             # Daemon process ID (standalone mode)
├── daemon.sock            # Unix domain socket for IPC
├── daemon.log             # Daemon stderr log
├── templates/             # Global prompt templates
├── agents/                # Global agent definitions
├── swarms/                # Global swarm definitions
├── schedules/             # Global schedule definitions
└── triggers/              # Global trigger definitions
```

### Runtime files

| File | Purpose | When present |
|---|---|---|
| `daemon.pid` | PID file to prevent duplicate daemons | Standalone mode only |
| `daemon.sock` | Unix socket for CLI-to-daemon communication | While daemon is running |
| `daemon.log` | Daemon stderr output for debugging | While daemon is running |

### CLI binary

The macOS app installs the `pu` binary to `~/.pu/bin/pu` on launch. Add it to your PATH:

```sh
export PATH="$HOME/.pu/bin:$PATH"
```

## ID formats

| Entity | Format | Example |
|---|---|---|
| Worktree | `wt-{8 alphanumeric}` | `wt-abc12def` |
| Agent | `ag-{8 alphanumeric}` | `ag-xyz98765` |
| Session | UUID v4 | `550e8400-e29b-41d4-a716-446655440000` |

Character set: `[a-z0-9]` (36 characters).

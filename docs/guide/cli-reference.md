# CLI Reference

`pu` is the command-line interface to PurePoint. All commands support `--json` for machine-readable output.

## Commands

| Command | Description |
|---|---|
| [`pu init`](#init) | Initialize a PurePoint workspace |
| [`pu spawn`](#spawn) | Spawn an agent in a new worktree |
| [`pu status`](#status) | Show workspace status |
| [`pu bench`](#bench) | Suspend agents |
| [`pu play`](#play) | Resume a suspended agent |
| [`pu kill`](#kill) | Kill agents |
| [`pu attach`](#attach) | Attach to an agent's terminal |
| [`pu logs`](#logs) | View agent output logs |
| [`pu send`](#send) | Send text or keys to an agent |
| [`pu health`](#health) | Check daemon health |
| [`pu pulse`](#pulse) | Workspace overview at a glance |
| [`pu diff`](#diff) | Show git diffs across worktrees |
| [`pu watch`](#watch) | Live dashboard |
| [`pu clean`](#clean) | Remove worktrees and branches |
| [`pu prompt`](#prompt) | Manage saved prompt templates |
| [`pu agent`](#agent) | Manage saved agent definitions |
| [`pu swarm`](#swarm) | Manage and run swarm compositions |
| [`pu grid`](#grid) | Control the pane grid layout |
| [`pu schedule`](#schedule) | Manage scheduled tasks |
| [`pu trigger`](#trigger) | Manage event-driven triggers |
| [`pu gate`](#gate) | Evaluate git hook gates |

## init

Initialize a PurePoint workspace. Creates `.pu/` with manifest and config.

```sh
pu init [--json]
```

## spawn

Spawn an agent in a new or existing worktree.

```sh
pu spawn [prompt] [options]
```

| Flag | Description |
|---|---|
| `prompt` | The prompt text (optional if `--template` or `--file` provided) |
| `-a, --agent <type>` | Agent type: claude (default), codex, opencode, terminal |
| `-n, --name <name>` | Worktree name (becomes branch `pu/{name}`) |
| `-b, --base <branch>` | Base branch to fork from |
| `--root` | Spawn in project root, no worktree (conflicts with `--worktree`) |
| `-w, --worktree <id>` | Add agent to existing worktree (conflicts with `--root`) |
| `--template <name>` | Use a saved prompt template (conflicts with `--file`) |
| `--file <path>` | Read prompt from a file (conflicts with `--template`) |
| `--command <cmd>` | Command to run (for terminal agents) |
| `--var KEY=VALUE` | Variable substitution (repeatable) |
| `--no-auto` | Skip default auto-mode flags |
| `--agent-args <args>` | Extra flags passed to the agent CLI |
| `--plan` | Launch in plan/architect mode (read-only) |
| `--no-trigger` | Disable event triggers for this agent |
| `--json` | Machine-readable output |

### Examples

```sh
pu spawn "fix the auth bug" --name fix-auth
pu spawn "refactor tests" --agent codex --name refactor
pu spawn --root "research the codebase"
pu spawn --root --agent terminal
pu spawn --root --agent terminal --command "npm run dev"
pu spawn --template code-review --var BRANCH=main
pu spawn --file prompt.md --name task1
pu spawn "review code" --plan --name review
```

## status

Show workspace status.

```sh
pu status [--agent <id>] [--json]
```

## bench

Suspend (bench) agents. Pauses the agent process.

```sh
pu bench [agent_id] [--all] [--json]
```

When using `--all`, the invoking agent may also be suspended. Use `pu play` to resume.

## play

Resume a suspended agent.

```sh
pu play <agent_id> [--json]
```

## kill

Kill agents.

```sh
pu kill [options]
```

| Flag | Description |
|---|---|
| `--agent <id>` | Kill a specific agent (exclusive with `--worktree`, `--all`) |
| `-w, --worktree <id>` | Kill all agents in a worktree (exclusive with `--all`) |
| `--all` | Kill all worktree agents |
| `--include-root` | Also kill root agents (requires `--all`) |
| `--json` | Machine-readable output |

By default, `--all` preserves root agents (point guards). Add `--include-root` to kill everything.

## attach

Attach to an agent's terminal for interactive use.

```sh
pu attach <agent_id>
```

Press `Ctrl+C` to detach without killing the agent.

## logs

View agent output logs.

```sh
pu logs <agent_id> [--tail <bytes>] [--json]
```

Default tail: 500 bytes. Increase for more context: `--tail 5000`.

## send

Send text or keys to an agent's terminal.

```sh
pu send <agent_id> [text] [--no-enter] [--keys <sequence>] [--json]
```

| Flag | Description |
|---|---|
| `text` | Text to send (appends Enter by default) |
| `--no-enter` | Don't append Enter after text |
| `--keys <seq>` | Send control key sequence (e.g., `C-c`, `C-d`) |

## health

Check daemon health.

```sh
pu health [--json]
```

## pulse

Workspace pulse: agents, runtimes, and git stats at a glance.

```sh
pu pulse [--json]
```

## diff

Show git diffs across agent worktrees.

```sh
pu diff [--worktree <id>] [--stat] [--json]
```

| Flag | Description |
|---|---|
| `--worktree <id>` | Diff a specific worktree |
| `--stat` | Show file summary instead of full diff |

## watch

Live TUI dashboard showing all agents in real-time.

```sh
pu watch [--interval <ms>]
```

Default refresh: 800ms. Alt-screen mode. Press `Ctrl+C` to exit.

Shows all agents with status, elapsed time, prompt preview, and a summary bar.

## clean

Remove worktrees, kill their agents, and delete branches.

```sh
pu clean [--worktree <id>] [--all] [--json]
```

## prompt

Manage saved prompt templates.

```sh
pu prompt list [--json]
pu prompt show <name> [--json]
pu prompt create <name> --body <text> [--description <desc>] [--agent <type>] [--scope local|global] [--json]
pu prompt delete <name> [--scope local|global] [--json]
```

## agent

Manage saved agent definitions.

```sh
pu agent list [--json]
pu agent show <name> [--json]
pu agent create <name> [--agent-type <type>] [--template <name>] [--inline-prompt <text>] [--command <cmd>] [--tags <csv>] [--scope local|global] [--json]
pu agent delete <name> [--scope local|global] [--json]
```

## swarm

Manage and run swarm compositions.

```sh
pu swarm list [--json]
pu swarm show <name> [--json]
pu swarm create <name> --roster AGENT:ROLE:QTY [--worktrees <n>] [--worktree-template <tpl>] [--include-terminal] [--scope local|global] [--json]
pu swarm delete <name> [--scope local|global] [--json]
pu swarm run <name> [--var KEY=VALUE] [--json]
```

The `--roster` flag is repeatable. Format: `agent_def_name:role:quantity`.

## grid

Control the macOS app's pane grid layout.

```sh
pu grid show [--json]
pu grid split [--axis v|h] [--leaf <id>]
pu grid close [--leaf <id>]
pu grid focus [--direction up|down|left|right] [--leaf <id>]
pu grid assign <agent_id> [--leaf <id>]
```

## schedule

Manage scheduled tasks.

```sh
pu schedule list [--json]
pu schedule show <name> [--json]
pu schedule create <name> --start-at <datetime> --trigger <type> [options] [--json]
pu schedule delete <name> [--scope local|global] [--json]
pu schedule enable <name> [--json]
pu schedule disable <name> [--json]
```

### Create options

| Flag | Description |
|---|---|
| `--recurrence <type>` | none (default), hourly, daily, weekdays, weekly, monthly |
| `--start-at <datetime>` | Start time (RFC 3339 or YYYY-MM-DDTHH:MM:SS) |
| `--trigger <type>` | Trigger: agent-def, swarm-def, inline-prompt |
| `--trigger-name <name>` | Name of agent def or swarm def |
| `--trigger-prompt <text>` | Prompt text for inline-prompt trigger |
| `--agent <type>` | Agent type for inline-prompt (default: claude) |
| `--var KEY=VALUE` | Variable substitution (repeatable) |
| `--root` | Spawn in project root |
| `--name <name>` | Worktree name (required when not `--root`) |
| `--scope local\|global` | Definition scope |

## trigger

Manage event-driven triggers.

```sh
pu trigger list [--json]
pu trigger show <name> [--json]
pu trigger create <name> --on <event> [--inject <text>] [--gate <cmd>] [--scope local|global] [--json]
pu trigger delete <name> [--scope local|global] [--json]
```

| Flag | Description |
|---|---|
| `--on <event>` | Event: agent_idle, pre_commit, pre_push |
| `--inject <text>` | Text to inject (repeatable, creates sequence steps) |
| `--gate <cmd>` | Gate command (repeatable, creates gate-only steps) |
| `--description <text>` | Trigger description |

## gate

Evaluate git hook gates. Called by git hooks installed in worktrees.

```sh
pu gate <event> [--project-root <path>]
```

Events: `pre-commit`, `pre-push`.

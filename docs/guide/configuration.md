# Configuration

PurePoint is configured through `.pu/config.yaml` in your project root. Created by `pu init`.

## Default config

```yaml
defaultAgent: claude
envFiles:
  - .env
  - .env.local
```

Agents not listed in the config use built-in defaults. The full schema is documented in the [Config Schema Reference](../reference/config-schema.md).

## Agent types

PurePoint ships with four built-in agent types. Each launches with its bare
command: PurePoint adds no permission, sandbox or auto-approve flags of its own.

| Type | CLI tool | Launches as | Description |
|---|---|---|---|
| `claude` | Claude Code | `claude` | Claude Code CLI |
| `codex` | Codex | `codex` | OpenAI Codex CLI |
| `opencode` | OpenCode | `opencode` | OpenCode CLI |
| `terminal` | `shell` (resolves to `$SHELL`) | `$SHELL` | Plain terminal |

Each tool then follows its own defaults and settings, so Claude Code and Codex
ask before running commands or editing files unless you configure them not to.
Worktrees isolate git state, not the filesystem or network, so an agent that
skips approvals can reach anything your user account can.

The default agent type is `claude`. Change it with:

```yaml
defaultAgent: codex
```

Or per-spawn with `--agent`:

```sh
pu spawn "fix bug" --agent codex
```

## Customizing launch args

`launchArgs` is one free-form string appended to the agent's command. It is
split like a shell command line, so quotes group words. PurePoint does not
interpret the flags, so new or renamed CLI options work without a PurePoint
update.

```yaml
agents:
  claude:
    name: claude
    command: claude
    launchArgs: "--model opus --append-system-prompt 'Keep answers short.'"
```

The older list form (`launchArgs: ["--model", "opus"]`) still loads. When
PurePoint writes the config (for example from Settings), it saves the string
form.

### Your own default for every project (auto mode)

To opt into auto mode yourself without changing any project, set machine-level
launch args in `~/.pu/agent-settings.yaml`, or in the macOS app under
**Settings → Agents → Machine Settings → Launch args for all projects**:

```yaml
launchArgs:
  claude: "--dangerously-skip-permissions"
  codex: "--sandbox workspace-write --ask-for-approval on-request"
```

These apply to every project on this machine that doesn't set its own
`launchArgs`, including projects created later. Changes apply to new and resumed
agents without restarting the daemon; restart running agents to apply them.
A malformed file (bad YAML or an unbalanced quote) makes config resolution fail
with an error. PurePoint never falls back to permission-skipping flags.

Precedence, highest first:

1. `pu spawn --no-auto`: the bare command, ignoring all configured launch args
2. Project `launchArgs` in `.pu/config.yaml`, when set (even to `""`)
3. Machine-level `launchArgs` in `~/.pu/agent-settings.yaml`
4. Nothing: the bare command

`--agent-args` is appended after whichever of these applies.

### Codex YOLO for all projects

To run Codex with full access and no command approvals across every project on
this Mac, open **Settings → Agents** and enable **YOLO for all projects**
(`codexYolo: true` in `~/.pu/agent-settings.yaml`). New and resumed Codex agents
launch with `--dangerously-bypass-approvals-and-sandbox --no-daemon`, replacing
any sandbox and approval options in the effective launch args. Turning it off
restores the project and machine settings. The switch is available even without
a project open.

### Additional agent config fields

| Field | Type | Default | Description |
|---|---|---|---|
| `name` | string | (required) | Agent type identifier |
| `command` | string | (required) | CLI binary to run (`claude`, `codex`, `opencode`, `shell`) |
| `launchArgs` | string (or legacy string[]) | unset | Extra args appended to the command |
| `promptFlag` | string | `"-p"` | Flag used to pass the prompt text to the agent CLI |
| `interactive` | bool | `true` | Whether the agent is interactive (affects PTY allocation) |

### Launching bare in one project

In Settings, an empty project field means "use the launch args for all
projects". To launch the bare command in one project even though you set
machine-level args, set `launchArgs` to an empty string in its config:

```yaml
agents:
  claude:
    name: claude
    command: claude
    launchArgs: ""    # bare `claude`, even if agent-settings.yaml sets args
```

### Per-spawn override

Use `--no-auto` to launch the bare command for a single spawn, ignoring
project and machine launch args:

```sh
pu spawn "careful review" --no-auto
```

Or pass extra args directly:

```sh
pu spawn "review" --agent-args "--model opus --effort high"
```

## Environment files

PurePoint loads environment files into agent processes:

```yaml
envFiles:
  - .env
  - .env.local
```

Files are loaded in order; later files override earlier ones.

## Scope

Configuration is project-level (`.pu/config.yaml`), apart from the machine settings in `~/.pu/agent-settings.yaml` (launch args for all projects, Codex YOLO, agent status tracking). For other cross-project defaults, use global definitions (templates, agent defs, swarms, schedules, triggers in `~/.pu/`).

See [Concepts: Scope](concepts.md#scope) for how local and global definitions interact.

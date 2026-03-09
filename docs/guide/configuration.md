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

PurePoint ships with four built-in agent types:

| Type | CLI tool | Default auto-mode | Description |
|---|---|---|---|
| `claude` | Claude Code | `--dangerously-skip-permissions` | Claude Code CLI |
| `codex` | Codex | `--full-auto` | OpenAI Codex CLI |
| `opencode` | OpenCode | (none) | OpenCode CLI |
| `terminal` | `shell` (resolves to `$SHELL`) | (none) | Plain terminal |

The default agent type is `claude`. Change it with:

```yaml
defaultAgent: codex
```

Or per-spawn with `--agent`:

```sh
pu spawn "fix bug" --agent codex
```

## Customizing launch args

Each agent type has default launch arguments. Override them in `config.yaml`:

```yaml
agents:
  claude:
    name: claude
    command: claude
    launchArgs:
      - "--dangerously-skip-permissions"
      - "--model"
      - "opus"
```

### Additional agent config fields

| Field | Type | Default | Description |
|---|---|---|---|
| `name` | string | (required) | Agent type identifier |
| `command` | string | (required) | CLI binary to run (`claude`, `codex`, `opencode`, `shell`) |
| `launchArgs` | string[] | varies | Default flags passed to the agent CLI |
| `promptFlag` | string | `"-p"` | Flag used to pass the prompt text to the agent CLI |
| `interactive` | bool | `true` | Whether the agent is interactive (affects PTY allocation) |

### Disabling auto-mode

Set `launchArgs` to an empty array to disable auto-mode entirely:

```yaml
agents:
  claude:
    name: claude
    command: claude
    launchArgs: []    # Will prompt for permissions
```

### Per-spawn override

Use `--no-auto` to skip default launch args for a single spawn:

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

Configuration is project-level only (`.pu/config.yaml`). For cross-project defaults, use global definitions (templates, agent defs, swarms, schedules, triggers in `~/.pu/`).

See [Concepts: Scope](concepts.md#scope) for how local and global definitions interact.

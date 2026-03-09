# Config Schema

PurePoint project configuration lives at `.pu/config.yaml`. Created by `pu init` with sensible defaults.

## File format

```yaml
defaultAgent: claude
envFiles:
  - .env
  - .env.local
agents:
  claude:
    name: claude
    command: claude
    launchArgs:
      - "--dangerously-skip-permissions"
```

## Fields

| Field | Type | Default | Description |
|---|---|---|---|
| `defaultAgent` | string | `"claude"` | Agent type used when `--agent` is not specified |
| `envFiles` | string[] | `[".env", ".env.local"]` | Environment files loaded into agent processes |
| `agents` | map | (built-in defaults) | Per-agent configuration; see below |

## Agent configuration

Each entry in `agents` configures a specific agent type.

| Field | Type | Default | Description |
|---|---|---|---|
| `name` | string | (required) | Agent type identifier |
| `command` | string | (required) | Binary to execute |
| `promptFlag` | string | `null` | CLI flag for prompt injection (e.g., `"--prompt"`) |
| `interactive` | bool | `true` | Whether the agent runs in an interactive PTY |
| `launchArgs` | string[] or null | `null` | CLI arguments; see resolution logic below |

### Built-in agent defaults

These defaults apply when an agent type is not defined in `config.yaml`:

| Agent | Command | Default `launchArgs` |
|---|---|---|
| `claude` | `claude` | `["--dangerously-skip-permissions"]` |
| `codex` | `codex` | `["--full-auto"]` |
| `opencode` | `opencode` | `[]` |
| `terminal` | `shell` (resolved to `$SHELL`) | `[]` |

### Launch args resolution

The `launchArgs` field has three-state semantics:

| Value | Behavior |
|---|---|
| `null` (omitted) | Use built-in defaults for this agent type |
| `[]` (empty array) | No launch args; explicitly disables auto-mode |
| `["--flag", ...]` | Use exactly these args, replacing defaults |

### Example: customize Claude flags

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

### Example: disable auto-mode for Claude

```yaml
agents:
  claude:
    name: claude
    command: claude
    launchArgs: []    # No --dangerously-skip-permissions
```

## Config merging

Configuration is resolved in this order (later overrides earlier):

1. **Built-in defaults** (compiled into `pu`)
2. **Project config** (`.pu/config.yaml`)
3. **CLI flags** (`--agent`, `--no-auto`, `--agent-args`)

Agent types not defined in the config file are filled in from built-in defaults. If your config only defines `codex`, the `claude`, `opencode`, and `terminal` types still work with their default settings.

## Available agent flags

The default config written by `pu init` includes commented-out examples of useful flags per agent type:

### Claude

```
--dangerously-skip-permissions    # Skip permission prompts
--permission-mode <mode>          # default, acceptEdits, plan, bypassPermissions
--model <model>                   # sonnet, opus, haiku
--effort <level>                  # low, medium, high
--allowedTools <tools...>         # Restrict available tools
--append-system-prompt <prompt>   # Add to system prompt
--max-budget-usd <amount>         # Spending limit
```

### Codex

```
--full-auto                       # No approval required
-a <mode>                         # Approval: untrusted, on-request, never
--model <model>                   # Model to use
```

### OpenCode

```
--model <provider/model>          # Model to use
--variant <effort>                # Effort level
```

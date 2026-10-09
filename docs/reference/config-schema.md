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

## Machine-wide Codex permissions

Settings → Agents → **YOLO for all projects** stores this preference in
`~/.pu/agent-settings.yaml`:

```yaml
codexYolo: true
```

The default is `false`. When enabled, the daemon resolves Codex configuration
with `--dangerously-bypass-approvals-and-sandbox --no-daemon`, replacing sandbox
and approval options in `command` and `launchArgs`. Wrapper prefixes and other
arguments, including model and search, are preserved.
For an encapsulated wrapper with no explicit `codex` executable token, the full
command is preserved and only `launchArgs` are normalized. Claude and OpenCode are
unaffected. This preference applies on every spawn and resume, including projects
created later, without rewriting project files or requiring a sync action.
Running agents need a restart. Turning it off restores each project's own
configuration. Malformed global settings produce a config error.

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
| `codex` | `codex` | `["--sandbox", "workspace-write", "--ask-for-approval", "on-request"]` |
| `opencode` | `opencode` | `[]` |
| `terminal` | `shell` (resolved to `$SHELL`) | `[]` |

### Launch args resolution

The `launchArgs` field has three-state semantics:

| Value | Behavior |
|---|---|
| `null` (omitted) | Use built-in defaults for this agent type |
| `[]` (empty array) | No launch args; explicitly disables auto-mode |
| `["--flag", ...]` | Use exactly these args, replacing defaults |

On spawn and resume, launch args are appended after arguments in `command`.
Resume keeps the configured executable and wrapper prefix, then adds the agent's
resume arguments (`--resume SESSION_ID`, `resume --last`, or `--continue`).
On spawn, launch args are followed by
`--agent-args` and the startup prompt. For a wrapper, use e.g.
`command: scripts/agent-launch.sh codex` and keep agent flags in `launchArgs`.
Arguments retain their order and repeated values; avoid defining the same CLI
option in both `command` and `launchArgs`.

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
-s <mode>                         # Sandbox: read-only, workspace-write, danger-full-access
-a <mode>                         # Approval: untrusted, on-request, never
--model <model>                   # Model to use
--search                          # Enable live web search
```

### OpenCode

```
--model <provider/model>          # Model to use
--variant <effort>                # Effort level
```

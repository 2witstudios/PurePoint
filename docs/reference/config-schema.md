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
    launchArgs: "--model opus"
```

## Machine settings (`~/.pu/agent-settings.yaml`)

```yaml
codexYolo: false
agentStatusTracking: true
launchArgs:
  claude: "--dangerously-skip-permissions"
```

| Field | Type | Default | Description |
|---|---|---|---|
| `codexYolo` | bool | `false` | Codex YOLO for all projects; see below |
| `agentStatusTracking` | bool | `true` | Report working/idle/question status through native adapters |
| `launchArgs` | map of agent type → string (or string[]) | `{}` | Launch args for every project that leaves `launchArgs` unset |

The file is read on every config resolution, so spawn and resume pick up changes
without a daemon restart. It is written under a lock with atomic replacement.
Malformed content, including an unbalanced quote in a `launchArgs` string, is a
config error; it never falls back to permission flags. The keys under
`launchArgs` are agent types (`claude`, `codex`, `opencode`, `terminal`).

## Machine-wide Codex permissions

Settings → Agents → **YOLO for all projects** stores this preference in
`~/.pu/agent-settings.yaml`:

```yaml
codexYolo: true
```

The default is `false`. When enabled, the daemon resolves Codex configuration
with `--dangerously-bypass-approvals-and-sandbox --no-daemon`, replacing sandbox
and approval options in `command` and the effective launch args (project, else
machine-level). Wrapper prefixes and other
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
| `launchArgs` | string, string[] or null | `null` | Extra CLI arguments; see resolution logic below |

### Built-in agent defaults

These defaults apply when an agent type is not defined in `config.yaml`. No
agent gets launch args by default:

| Agent | Command | Default `launchArgs` |
|---|---|---|
| `claude` | `claude` | none |
| `codex` | `codex` | none |
| `opencode` | `opencode` | none |
| `terminal` | `shell` (resolved to `$SHELL`) | none |

### Launch args format

`launchArgs` is a free-form string split shell-style: whitespace separates
arguments, and single quotes, double quotes and backslashes group them, e.g.
`--append-system-prompt "be terse"` is two arguments. Nothing is expanded
(no variables, globs or command substitution). An unbalanced quote is a config
error. The legacy list form (`["--model", "opus"]`) is still accepted;
PurePoint writes the string form when it saves the config. For Codex, a legacy
`--full-auto` or `--approval-mode=full-auto` that you wrote is rewritten to
`--sandbox workspace-write --ask-for-approval on-request`; no other flags are
added.

### Launch args resolution

| Project `launchArgs` | Machine `launchArgs.<agent>` | Launched with |
|---|---|---|
| `null` (omitted) | unset | the bare command |
| `null` (omitted) | `"--flag ..."` | the machine args |
| `""` or `[]` | any | the bare command (project opts out) |
| `"--flag ..."` | any | exactly the project args |

`pu spawn --no-auto` ignores both and launches the bare command, including when the agent is later resumed.

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
    launchArgs: "--model opus --append-system-prompt 'Keep answers short.'"
```

### Example: opt into auto mode for all your projects

```yaml
# ~/.pu/agent-settings.yaml
launchArgs:
  claude: "--dangerously-skip-permissions"
```

### Example: ignore the machine default in one project

```yaml
agents:
  claude:
    name: claude
    command: claude
    launchArgs: ""    # bare `claude`
```

## Config merging

Configuration is resolved in this order (later overrides earlier):

1. **Built-in defaults** (compiled into `pu`; no launch args)
2. **Machine settings** (`~/.pu/agent-settings.yaml` `launchArgs`)
3. **Project config** (`.pu/config.yaml`)
4. **CLI flags** (`--agent`, `--no-auto`, `--agent-args`)

Agent types not defined in the config file are filled in from built-in defaults. If your config only defines `codex`, the `claude`, `opencode`, and `terminal` types still work with their default settings.

## Available agent flags

These are the tools' own flags, listed for convenience; PurePoint passes whatever you write and does not validate them. Check each tool's `--help` for its current options. The default config written by `pu init` includes commented-out examples:

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

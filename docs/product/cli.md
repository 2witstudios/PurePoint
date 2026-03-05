# CLI

**Maturity: DECIDED** | ID Prefix: CLI | Dependencies: `architecture/ipc-api.md`

## Purpose

The `pu` command-line tool. A thin client that sends requests to the daemon and formats responses for terminal output. Zero domain logic — all state and operations live in the daemon.

## Conceptual Model

```
User types: pu {command} [args] [--flags]
  CLI parses args (clap)
  CLI ensures daemon is running (auto-start if needed)
  CLI connects to Unix socket (~/.pu/daemon.sock)
  CLI sends JSON request, reads JSON response
  CLI formats response for terminal (or raw JSON with --json)
  CLI exits with appropriate code
```

Key behaviors:
- Auto-starts daemon if not running
- Machine-readable output mode for conductor agents
- Ability to attach directly to an agent's terminal session

## Decisions

! [CLI-001] Auto-start polls 30x100ms (3s timeout), exits with error pointing to `~/.pu/daemon.log` — CLI calls `ensure_daemon()` which first checks health, then spawns `pu-engine` (found via `which`) as a detached process with stderr redirected to `~/.pu/daemon.log`. Polls `Request::Health` every 100ms up to 30 attempts. On timeout: `CliError::Other("daemon did not start within 3 seconds")`. Implemented in `pu-cli/src/daemon_ctrl.rs`.

! [CLI-002] `--json` flag on `status` command outputs raw JSON response — provides machine-readable output for conductor agents and scripts. Currently on `status` only (not a global flag). Implemented in `pu-cli/src/main.rs` (status command handler).

## Implemented Commands

| Command | Args/Flags | Description |
|---|---|---|
| `pu init` | `--json` | Register current project with daemon |
| `pu spawn <prompt>` | `--agent`, `--name`, `--base`, `--root`, `--worktree`, `--template`, `--file`, `--var KEY=VALUE`, `--json` | Spawn an agent (in worktree or root) |
| `pu status` | `--agent <id>`, `--json` | Show project/agent status |
| `pu kill` | `--agent`, `--worktree`, `--all` (mutually exclusive), `--json` | Kill agent(s) |
| `pu logs <agent_id>` | `--tail <n>` (default 500), `--json` | Tail agent output buffer |
| `pu attach <agent_id>` | — | Interactive PTY attach to agent |
| `pu health` | `--json` | Check daemon health |
| `pu send <agent_id> [text]` | `--no-enter`, `--keys <key>`, `--json` | Send text or control keys to agent terminal |
| `pu prompt list` | `--json` | List saved prompt templates |
| `pu grid show` | `--json` | Show current pane grid layout |
| `pu grid split` | `--axis <v\|h>`, `--leaf <id>` | Split a pane |
| `pu grid close` | `--leaf <id>` | Close a pane |
| `pu grid focus` | `--direction <up\|down\|left\|right>`, `--leaf <id>` | Move focus to another pane |
| `pu grid assign <agent_id>` | `--leaf <id>` | Assign an agent to a pane |

## Research Notes

**Client implementation (`pu-cli/src/client.rs`):** Connects to Unix socket, writes `{json}\n`, reads one newline-terminated response. 30-second request timeout. `ConnectionRefused`/`NotFound` errors converted to `DaemonNotRunning` error type.

**Daemon discovery:** Socket path resolved from `pu_core::paths::daemon_socket_path()` → `~/.pu/daemon.sock`. No environment variable override currently.

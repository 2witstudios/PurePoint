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

! [CLI-001] Auto-start with a 3s wall-clock startup budget, exits with error pointing to `~/.pu/daemon.log` — CLI calls `ensure_daemon()` which first checks health (up to three 2s-bounded probes, 200ms apart; a `BUSY` reply counts as a live daemon), then spawns `pu-engine` (found via `which`) as a detached process with stderr redirected to `~/.pu/daemon.log`. A redundant spawn is harmless: it exits on the daemon lock. Then polls `Request::Health` with exponential backoff (10ms doubling to 640ms) inside a 3s timeout that includes the probes. On timeout: `CliError::Other("daemon did not start within 3 seconds")`. Implemented in `pu-cli/src/daemon_ctrl.rs`.

! [CLI-002] `--json` flag for machine-readable output — provides raw JSON responses for conductor agents and scripts. Available on nearly every command: `init`, `spawn`, `status`, `bench`, `play`, `kill`, `logs`, `health`, `pulse`, `diff`, `clean`, and all CRUD subcommands (`prompt`, `agent`, `swarm`, `schedule`, `trigger`). Per-command flag (not global). Implemented across command handlers in `pu-cli/src/commands/`.

## Implemented Commands

| Command | Args/Flags | Description |
|---|---|---|
| `pu init` | `--json` | Register current project with daemon |
| `pu spawn [prompt]` | `--agent`, `--name`, `--base`, `--root`, `--worktree`, `--template`, `--file`, `--command`, `--var KEY=VALUE`, `--no-auto`, `--agent-args`, `--plan`, `--no-trigger`, `--trigger`, `--json` | Spawn an agent (in worktree or root) |
| `pu status` | `--global`, `--agent <id>`, `--json` | Show project, global summary, or agent status |
| `pu projects list` | `--global`, `--json` | List daemon-known projects |
| `pu agents list` | `--global`, `--state running\|suspended\|broken\|unknown`, `--json` | List agent instances with ownership |
| `pu worktrees list` | `--global`, `--json` | List worktrees with ownership |
| `pu bench [agent_id]` | `--all`, `--json` | Suspend (bench) agents |
| `pu play <agent_id>` | `--json` | Resume a benched agent |
| `pu kill` | `--agent`, `--worktree`, `--all` (mutually exclusive), `--include-root` (requires `--all`), `--json` | Kill agent(s) |
| `pu attach <agent_id>` | — | Interactive PTY attach to agent |
| `pu logs <agent_id>` | `--tail <n>` (default 500), `--json` | Tail agent output buffer |
| `pu send <agent_id> [text]` | `--no-enter`, `--keys <key>`, `--json` | Send text or control keys to agent terminal. Claude agents get screen-confirmed delivery (full text submitted as one turn, or non-zero exit); no nudge needed |
| `pu health` | `--json` | Check daemon health |
| `pu pulse` | `--json` | Workspace overview (agents, runtimes, git stats) |
| `pu diff` | `--worktree <id>`, `--stat`, `--json` | Show git diffs across worktrees |
| `pu watch` | `--interval <ms>` (default 800) | Live TUI dashboard |
| `pu clean` | `--worktree <id>`, `--all`, `--json` | Remove worktrees, agents, and branches |
| `pu prompt list` | `--json` | List saved prompt templates |
| `pu prompt show <name>` | `--json` | Show prompt template details |
| `pu prompt create <name>` | `--body`, `--description`, `--agent`, `--scope`, `--json` | Create prompt template |
| `pu prompt delete <name>` | `--scope`, `--json` | Delete prompt template |
| `pu agent list` | `--json` | List agent definitions |
| `pu agent show <name>` | `--json` | Show agent definition details |
| `pu agent create <name>` | `--agent-type`, `--template`, `--inline-prompt`, `--command`, `--tags`, `--scope`, `--json` | Create agent definition |
| `pu agent delete <name>` | `--scope`, `--json` | Delete agent definition |
| `pu swarm list` | `--json` | List swarm definitions |
| `pu swarm show <name>` | `--json` | Show swarm definition details |
| `pu swarm create <name>` | `--worktrees`, `--worktree-template`, `--roster AGENT:ROLE:QTY`, `--include-terminal`, `--scope`, `--json` | Create swarm definition |
| `pu swarm delete <name>` | `--scope`, `--json` | Delete swarm definition |
| `pu swarm run <name>` | `--var KEY=VALUE`, `--json` | Execute a swarm |
| `pu schedule list` | `--json` | List schedules |
| `pu schedule show <name>` | `--json` | Show schedule details |
| `pu schedule create <name>` | `--recurrence`, `--start-at`, `--trigger`, `--trigger-name`, `--trigger-prompt`, `--agent`, `--var KEY=VALUE`, `--scope`, `--json` | Create schedule |
| `pu schedule delete <name>` | `--scope`, `--json` | Delete schedule |
| `pu schedule enable <name>` | `--json` | Enable schedule |
| `pu schedule disable <name>` | `--json` | Disable schedule |
| `pu grid show` | `--json` | Show current pane grid layout |
| `pu grid split` | `--axis <v\|h>`, `--leaf <id>` | Split a pane |
| `pu grid close` | `--leaf <id>` | Close a pane |
| `pu grid focus` | `--direction <up\|down\|left\|right>`, `--leaf <id>` | Move focus to another pane |
| `pu grid assign <agent_id>` | `--leaf <id>` | Show an agent in a pane's active tab (default: focused pane) |
| `pu grid tab new` | `--leaf <id>`, `--agent <id>` | Open a tab after the active one (empty unless `--agent`) |
| `pu grid tab select [N]` | `--next`, `--prev`, `--leaf <id>` | Select tab by 1-based position or next/prev (exactly one) |
| `pu grid tab close` | `--leaf <id>`, `--tab <id>` | Close a tab (default: active tab of focused pane) |
| `pu grid tab move <tab_id>` | `--to <leaf>`, `--index <n>` | Move a tab to another pane (default: append) |
| `pu grid tab break` | `--tab <id>`, `--axis <v\|h>` | Move a tab into a new pane split off its pane |
| `pu grid <any but show>` | `--workspace <id>` | Act on that workspace instead of the one on screen (leaf and tab ids are only unique within a workspace) |
| `pu trigger list` | `--json` | List trigger definitions |
| `pu trigger show <name>` | `--json` | Show trigger details |
| `pu trigger create <name>` | `--on <event>`, `--inject`, `--gate`, `--description`, `--scope`, `--json` | Create trigger definition |
| `pu trigger delete <name>` | `--scope`, `--json` | Delete trigger definition |
| `pu trigger assign <agent_id> <trigger_name>` | `--json` | Assign trigger to idle agent |
| `pu gate <event>` | `--project-root` | Evaluate git hook gates (pre-commit, pre-push) |

## Research Notes

**Client implementation (`pu-cli/src/client.rs`):** Connects to Unix socket, writes `{json}\n`, reads one newline-terminated response. 30-second request timeout. `ConnectionRefused`/`NotFound` errors converted to `DaemonNotRunning` error type.

**Daemon discovery:** Socket path resolved from `pu_core::paths::daemon_socket_path()` → `~/.pu/daemon.sock`. No environment variable override currently.

## Global Inventory and Routing Contract

**Feature maturity: SPECIFIED.** Scope is the selected daemon's registered projects plus its standalone sessions. Registration survives restarts, with project manifests retaining durable agent/worktree state. The CLI never enumerates manifests to answer global queries.

- REQ-CLI-003: Given any cwd, global status should return totals and project summaries through one Inventory request, without git scans, logs or full prompts.
- REQ-CLI-004: Given any cwd, projects/agents/worktrees list should default to global scope; --project should constrain the inventory and --state should filter agent records without changing scope totals.
- REQ-CLI-005: Given a targeted agent ID without an explicit --project scope, status/kill/bench/play/trigger assign should resolve ownership in the daemon. Duplicate durable IDs should require project scope; a mismatched live owner should reject the operation.
- REQ-CLI-006: Given --project ROOT, project commands should prefer it over PU_PROJECT_ROOT and cwd; global session commands should validate that scope before acting. Existing channel/gate --project-root flags retain their explicit routing behavior.
- REQ-CLI-007: Given unreadable projects or unowned persisted agents, inventory should report project errors and unknown states instead of inventing running agents or an exhaustive zero. Known live sessions remain visible, including standalone shells.

Interfaces: global `--project ROOT` and `--socket PATH` flags; `Inventory { project_root?: string, kind: summary|projects|agents|worktrees, state?: running|suspended|broken|unknown }`; `ResolveAgent { agent_id, project_root?: string }`. Inventory returns observation time, scope, completeness, whole-scope counts, project availability, and requested flat records in deterministic order. Running counts derive from live session exit receivers; AI and terminal subtotals are separate. Legacy targeted requests with an empty project_root request daemon ownership resolution. Standalone shells have null ownership and support status/logs/input/attach/kill; resumable project operations return UNSUPPORTED_AGENT_OPERATION for them.

Global summary and list commands ignore PU_PROJECT_ROOT unless --project is supplied. Ordinary `pu status` remains project-scoped. Bulk operations retain project scope. `pu init` sends Init to the daemon so already-initialized projects also register. The CLI does not install global plugin files when connected to a custom socket.

Validation: `crates/pu-cli/tests/global_inventory.rs` exercises real CLI processes and IPC from an unrelated cwd across two projects, root/worktree agents, scoped rejection, send/logs, bench/kill, standalone shells and registry restart. Engine inventory tests cover aliases, corrupt stores, invalid registrations, duplicate IDs, partial inventories and exited handles before reaping.

The CLI verifies protocol v7 before daemon commands. An older or newer daemon produces an explicit incompatibility error with restart guidance; it is never automatically stopped because that would terminate active agents. A busy daemon reports BUSY until compatibility can be verified.

# CLI

**Maturity: DECIDED** | ID Prefix: CLI | Dependencies: `architecture/ipc-api.md`, `product/machines.md`

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

! [CLI-001] Auto-start with a 3s wall-clock startup budget, exits with error pointing to `~/.pu/daemon.log` — CLI calls `ensure_daemon()` which first checks health (up to three 2s-bounded probes, 200ms apart; a `BUSY` reply counts as a live daemon), then spawns `pu-engine` (a bundled `pu` uses only its app's `Contents/MacOS/pu-engine`; otherwise `PU_ENGINE_PATH` if set, else next to the `pu` executable, else `~/.pu/bin`; never a bare `PATH` search, and only an executable owned by the user or root that others can't write) in its own session (`setsid`, null stdin/stdout) so it outlives an SSH session that started it with stderr redirected to `~/.pu/daemon.log`. A redundant spawn is harmless: it exits on the daemon lock. Then polls `Request::Health` with exponential backoff (10ms doubling to 640ms) inside a 3s timeout that includes the probes. On timeout: `CliError::Other("daemon did not start within 3 seconds")`. Implemented in `pu-cli/src/daemon_ctrl.rs`.

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
| `pu read <agent_id>` | `--limit <n>` (default 50), `--since <cursor>`, `--session <exact-id>`, `--json` | Read exact native conversation or labeled terminal text |
| `pu logs <agent_id>` | `--tail <n>` (default 500), `--json` | Deprecated raw terminal diagnostic bytes |
| `pu send <agent_id> [text]` | `--no-enter`, `--keys <key>`, `--json` | Send text or control keys to agent terminal. Claude agents get screen-confirmed delivery (full text submitted as one turn, or non-zero exit); no nudge needed |
| `pu health` | `--json` | Check daemon health |
| `pu daemon proxy` | honours global `--socket` | Relay stdin/stdout to the daemon socket as one connection, for SSH exec channels. Never starts the daemon (see below) |
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
| `pu machine list` | `--json` | List machines registered in `~/.pu/machines.json` |
| `pu machine add <name> <ssh_host>` | `--json` | Register a machine (`~/.ssh/config` alias or `user@host[:port]`) |
| `pu machine remove <name>` | `--json` | Forget a machine; its daemon and agents keep running |
| `pu machine status [name]` | `--json` | Probe machine daemons over SSH: `running`, `busy`, `stopped`, `incompatible` or `unreachable`. Never starts a daemon |
| `pu machine start <name>` | — | Start a machine's daemon by running `pu health` there over ssh |
| `pu phone install` / `uninstall` | `--now`, `--port` | Linux only: write, enable (and start) or remove the `pu-pointguard.service` user unit for the payload matching this `pu` (LPH-003). Uninstall keeps device trust |
| `pu phone start` / `stop` / `restart` / `status` | `--force` (stop, restart), `--json` (status) | Service lifecycle. `stop`/`restart` ask the bridge to stop only while idle; `restart` moves to this `pu`'s payload and prunes older ones |
| `pu phone run` | `--port` | Run the bridge in the foreground with an empty, explicit environment (hosts without systemd) |
| `pu phone pair` | `--ttl <1-300>`, `--text` | Terminal QR (or `purepoint-pair:` paste code) for one single-use enrollment; revoked on any exit except success (LPH-007) |
| `pu phone devices` / `grant` / `rename` / `revoke` | `--json`; `--terminals off\|view\|control`, `--workspace on\|off` | Device administration by name or unique id prefix (≥ 4 characters). Grants apply downgrades first and print the derived chat level (LPH-013) |
| `pu phone login` / `model` / `cwd` | `[provider]`, `--api-key-stdin`; `[<provider>/<model>]`; `<path>` | Headless provider sign-in over the owner admin socket, model choice and working folder (LPH-009) |
| `pu --machine <name> <command>` | conflicts with `--socket` | Run a command against a machine's daemon (see below) |

## Research Notes

**Client implementation (`pu-cli/src/client.rs`):** Connects to Unix socket, writes `{json}\n`, reads one newline-terminated response. 30-second request timeout. `ConnectionRefused`/`NotFound` errors converted to `DaemonNotRunning` error type.

**Daemon discovery:** Socket path resolved from `pu_core::paths::daemon_socket_path()` → `~/.pu/daemon.sock`, overridden by the global `--socket PATH` or `--machine NAME` flags. No environment variable override for the socket.

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

The CLI verifies protocol v9 before daemon commands. An older or newer daemon produces an explicit incompatibility error with restart guidance; it is never automatically stopped because that would terminate active agents. A busy daemon reports BUSY until compatibility can be verified.


Conversation reads (protocol 8): `Read { project_root, agent_id, limit, since?, session_id? }` returns `ReadResult` with runtime, exact session ID, source kind, normalized records, cursor and completeness indicators. See [Agent Conversation Reads](../product/agent-read.md). Resume uses exact recorded native identity; legacy unbound workers require `pu read AGENT --session ID`. No newest-session selection.

## Machines (`--machine`)

See [Machines over SSH](machines.md) (MACH-003, MACH-006, MACH-008). `pu --machine <name> <command>` looks the name up case-insensitively in `~/.pu/machines.json` and talks to that machine's daemon:

- If the app's forward `~/.pu/machines/<id>/daemon.sock` answers `Health`, it is reused. Otherwise the CLI resolves the remote `$HOME` (`ssh -o BatchMode=yes <host> 'printf %s "$HOME"'`), opens a private `ssh -N -L ~/.pu/machines/<id>/cli-<pid>.sock:<home>/.pu/daemon.sock` forward, waits up to 10s for it, and kills ssh and removes the socket when the command ends. Sockets left by CLI processes that died are swept on the next run.
- The daemon is never auto-started. A stopped daemon fails with ``daemon not running on <name>; start it with `pu machine start <name>` ``; a protocol mismatch or unreachable host fails too. No local daemon is ever started.
- Local paths are never sent: project-scoped commands require `--project <root on that machine>` (`PU_PROJECT_ROOT` and cwd are ignored); agent-targeted commands (`status --agent`, `kill --agent`, `send`, `attach`, `read`, `logs`, `bench`, `play`) resolve the agent by id on that daemon.
- `init`, `channel` and `gate` work on local files and reject `--machine`. `spawn --template` fetches the template from the machine's daemon (`GetTemplate`); `spawn --file` still reads the local file. `prompt list` never falls back to local templates.
- The global plugin freshness check is skipped, as for `--socket`.
- `PU_SSH` overrides the ssh binary (default `/usr/bin/ssh`); used by tests.

Validation: `crates/pu-cli/tests/machines.rs` drives real CLI processes through a fake `PU_SSH` against an in-process daemon, including a stopped daemon (no local daemon started, no sockets left) and an unreachable host.

## Daemon proxy (`pu daemon proxy`)

See [Machines over SSH](machines.md) (MACH-003). Clients that cannot `ssh -L` a Unix socket (the iPad app) open an SSH exec channel running `pu daemon proxy`; each channel is exactly one daemon connection carrying newline-delimited JSON.

- Connects to the daemon socket (`~/.pu/daemon.sock`, or `--socket`) and copies stdin to the socket and socket output to stdout byte-for-byte. Stdin EOF half-closes the socket's write side and the proxy keeps draining replies; the daemon closing the connection ends the proxy with exit 0, even while stdin is still open. Stdout is flushed before exit.
- Never starts a daemon: `daemon_ctrl::ensure_daemon` is not on this path. A missing or refusing socket exits 1 with ``error: cannot connect to the daemon at <socket> (<os error>); start it with `pu daemon start` `` on stderr.
- Stdout carries only daemon bytes. Every diagnostic goes to stderr, and the plugin freshness check is skipped so the proxy has no side work.
- Implemented in `crates/pu-cli/src/commands/daemon_proxy.rs`.

Validation: unit tests in `daemon_proxy.rs` relay 256 KiB both ways through an echo daemon, check half-close draining and daemon-side close; `crates/pu-cli/tests/daemon_proxy.rs` runs the real binary against a fake protocol daemon and the in-process `pu-engine` daemon (a `{"type":"health"}` line yields exactly the health response line) and against a missing socket (non-zero exit, empty stdout, no daemon started).

# IPC API Reference

The PurePoint daemon communicates with clients (CLI, macOS app) via NDJSON over a Unix domain socket.

## Transport

| Property | Value |
|---|---|
| Socket path | `~/.pu/daemon.sock` |
| Protocol version | 5 |
| Framing | Newline-delimited JSON (one JSON object per line) |
| Max message size | 1 MB (1,048,576 bytes) |
| Max connections | 64 (semaphore-limited) |
| Client timeout | 30 seconds |
| Binary data | Hex-encoded within JSON |

## Request format

All requests are JSON objects with a `"type"` field (snake_case) identifying the operation:

```json
{"type": "health"}
{"type": "spawn", "project_root": "/path", "prompt": "fix the bug", "agent": "claude"}
```

## Response format

Responses use the same `"type"` field convention. Error responses have `"type": "error"`:

```json
{"type": "error", "code": "AGENT_NOT_FOUND", "message": "no agent with id ag-abc"}
```

## Error codes

| Code | When |
|---|---|
| `PARSE_ERROR` | Malformed JSON request |
| `AGENT_NOT_FOUND` | Agent ID does not exist |
| `WORKTREE_NOT_FOUND` | Worktree ID does not exist |
| `NOT_FOUND` | Template, agent def, swarm def, schedule, or trigger not found |
| `NOT_INITIALIZED` | Project not initialized (run `pu init`) |
| `ALREADY_INITIALIZED` | Project already initialized |
| `DAEMON_NOT_RUNNING` | Daemon is not running |
| `DAEMON_CONNECTION_FAILED` | Connection to daemon failed |
| `PROTOCOL_MISMATCH` | Client/daemon protocol version mismatch |
| `MANIFEST_LOCKED` | Another process is writing to manifest |
| `IO_ERROR` | File I/O failure |
| `JSON_ERROR` | JSON parse/serialization error |
| `YAML_ERROR` | YAML parse/serialization error |
| `INTERNAL_ERROR` | Unexpected internal error |
| `SPAWN_FAILED` | Agent spawn failed |
| `KILL_TARGET_REQUIRED` | Kill command requires --agent, --worktree, or --all |
| `INVALID_ARGUMENT` | Invalid argument provided |

## Core operations

### Health

Check daemon liveness and protocol version.

**Request**: `{"type": "health"}`

**Response**:
```json
{
  "type": "health_report",
  "pid": 12345,
  "uptime_seconds": 3600,
  "protocol_version": 4,
  "projects": ["/path/to/project"],
  "agent_count": 3
}
```

### Init

Register a project and create `.pu/manifest.json`.

**Request**: `{"type": "init", "project_root": "/path/to/project"}`

**Response**: `{"type": "init_result", "created": true}`

### Shutdown

Gracefully shut down the daemon.

**Request**: `{"type": "shutdown"}`

**Response**: `{"type": "shutting_down"}`

### GetConfig

Read agent configuration for a project.

**Request**: `{"type": "get_config", "project_root": "/path"}`

**Response**:
```json
{
  "type": "config_report",
  "default_agent": "claude",
  "agents": {"claude": {"name": "claude", "command": "claude", ...}}
}
```

### UpdateAgentConfig

Update launch args for an agent type.

**Request**: `{"type": "update_agent_config", "project_root": "/path", "agent_name": "claude", "launch_args": ["--verbose"]}`

**Response**: `{"type": "ok"}`

## Agent lifecycle

### Spawn

Spawn a new agent, optionally in a new worktree.

**Request**:
```json
{
  "type": "spawn",
  "project_root": "/path",
  "prompt": "fix the bug",
  "agent": "claude",
  "name": "fix-auth",
  "base": "main",
  "root": false,
  "worktree": null,
  "command": null,
  "no_auto": false,
  "extra_args": null,
  "plan_mode": false,
  "no_trigger": false,
  "trigger": null
}
```

The `trigger` field optionally binds a trigger by name to the spawned agent.

**Response**:
```json
{
  "type": "spawn_result",
  "worktree_id": "wt-abc12345",
  "agent_id": "ag-xyz98765",
  "status": {"streaming": null}
}
```

The `status` field is an `AgentStatus` enum, not a string. See Agent Status section for all variants.

### Status

Get workspace or single-agent status.

**Request**: `{"type": "status", "project_root": "/path"}` or `{"type": "status", "project_root": "/path", "agent_id": "ag-xyz"}`

**Response** (workspace):
```json
{
  "type": "status_report",
  "worktrees": [...],
  "agents": [...]
}
```

### Kill

Kill one or more agents.

**Request**:
```json
{
  "type": "kill",
  "project_root": "/path",
  "target": {"type": "agent", "id": "ag-xyz"},
  "exclude": []
}
```

KillTarget variants: `{"agent": "..."}`, `{"worktree": "..."}`, `{"all": null}`, `{"all_worktrees": null}`

- `all` kills all agents including root project agents
- `all_worktrees` kills only worktree agents (excludes root)

**Response**:
```json
{
  "type": "kill_result",
  "killed": ["ag-xyz"],
  "exit_codes": {"ag-xyz": 0},
  "skipped": []
}
```

### Suspend

Suspend (bench) agents.

**Request**: `{"type": "suspend", "project_root": "/path", "target": {"type": "agent", "id": "ag-xyz"}}`

SuspendTarget variants: `{"type": "agent", "id": "..."}`, `{"type": "all"}`

**Response**: `{"type": "suspend_result", "suspended": ["ag-xyz"]}`

### Resume

Resume a suspended agent.

**Request**: `{"type": "resume", "project_root": "/path", "agent_id": "ag-xyz"}`

**Response**: `{"type": "resume_result", "agent_id": "ag-xyz", "status": "streaming"}`

### Rename

Rename an agent.

**Request**: `{"type": "rename", "project_root": "/path", "agent_id": "ag-xyz", "name": "new-name"}`

**Response**: `{"type": "rename_result", "agent_id": "ag-xyz", "name": "new-name"}`

### Diff

Get git diffs across worktrees.

**Request**: `{"type": "diff", "project_root": "/path", "worktree_id": null, "stat": false}`

**Response**: `{"type": "diff_result", "diffs": [...]}`

### Pulse

Get a real-time workspace overview.

**Request**: `{"type": "pulse", "project_root": "/path"}`

**Response**: `{"type": "pulse_report", "worktrees": [...], "root_agents": [...]}`

## Shell Spawning

### SpawnShell

Spawn an interactive shell in a directory (used by desktop app for terminal panes).

**Request**: `{"type": "spawn_shell", "cwd": "/path/to/directory"}`

**Response**:
```json
{
  "type": "spawn_result",
  "worktree_id": null,
  "agent_id": "ag-xyz98765",
  "status": {"streaming": null}
}
```

## PTY I/O

### Logs

Read the last N bytes of an agent's PTY output.

**Request**: `{"type": "logs", "agent_id": "ag-xyz", "tail": 500}`

**Response**: `{"type": "logs_result", "agent_id": "ag-xyz", "data": "hex-encoded-bytes"}`

### Attach

Enter streaming mode for an agent's terminal.

**Request**: `{"type": "attach", "agent_id": "ag-xyz"}`

**Response**: `{"type": "attach_ready", "buffered_bytes": 4096}`

After `attach_ready`, the server streams `Output` messages:
```json
{"type": "output", "agent_id": "ag-xyz", "data": "hex-encoded-bytes"}
```

The client can send `Input` and `Resize` messages on the same connection.

### Input

Send input to an agent's PTY.

**Request**: `{"type": "input", "agent_id": "ag-xyz", "data": "hex-encoded-bytes", "submit": true}`

**Response**: `{"type": "ok"}`

### Resize

Resize an agent's PTY.

**Request**: `{"type": "resize", "agent_id": "ag-xyz", "cols": 120, "rows": 40}`

**Response**: `{"type": "ok"}`

## Subscriptions

### SubscribeGrid

Subscribe to pane grid layout changes.

**Request**: `{"type": "subscribe_grid", "project_root": "/path"}`

**Response**: `{"type": "grid_subscribed"}`

Then streams: `{"type": "grid_event", "project_root": "/path", "command": {...}}`

### SubscribeStatus

Subscribe to workspace status changes.

**Request**: `{"type": "subscribe_status", "project_root": "/path"}`

**Response**: `{"type": "status_subscribed"}`

Then streams: `{"type": "status_event", "worktrees": [...], "agents": [...]}`

## Grid commands

**Request**: `{"type": "grid_command", "project_root": "/path", "command": {...}}`

GridCommand variants:

| Command | Fields | Description |
|---|---|---|
| `split` | `leaf_id`, `axis` | Split a pane (axis: `"v"` or `"h"`, default `"v"`) |
| `close` | `leaf_id` | Close a pane |
| `focus` | `leaf_id`, `direction` | Move focus (direction: `"up"`, `"down"`, `"left"`, `"right"`) |
| `set_agent` | `leaf_id`, `agent_id` | Assign an agent to a pane |
| `get_layout` | (none) | Get current layout |

Note: `leaf_id` and `direction` are optional fields.

## Worktree management

### CreateWorktree

**Request**: `{"type": "create_worktree", "project_root": "/path", "name": "feature", "base": "main"}`

**Response**: `{"type": "create_worktree_result", "worktree_id": "wt-abc12345"}`

### DeleteWorktree

**Request**: `{"type": "delete_worktree", "project_root": "/path", "worktree_id": "wt-abc12345"}`

**Response**:
```json
{
  "type": "delete_worktree_result",
  "worktree_id": "wt-abc12345",
  "killed_agents": ["ag-xyz"],
  "branch_deleted": true,
  "remote_deleted": false
}
```

## Template CRUD

| Operation | Request type | Response type | Key fields |
|---|---|---|---|
| List | `list_templates` | `template_list` | `project_root` |
| Get | `get_template` | `template_detail` | `project_root`, `name` |
| Save | `save_template` | `ok` | `project_root`, `name`, `description`, `agent`, `body`, `scope`, `command` (optional) |
| Delete | `delete_template` | `ok` | `project_root`, `name`, `scope` |

## Agent def CRUD

| Operation | Request type | Response type | Key fields |
|---|---|---|---|
| List | `list_agent_defs` | `agent_def_list` | `project_root` |
| Get | `get_agent_def` | `agent_def_detail` | `project_root`, `name` |
| Save | `save_agent_def` | `ok` | `project_root`, `name`, `agent_type`, `template`, `inline_prompt`, `tags`, `scope`, `available_in_command_dialog`, `icon`, `command` |
| Delete | `delete_agent_def` | `ok` | `project_root`, `name`, `scope` |

## Swarm def CRUD + execution

| Operation | Request type | Response type | Key fields |
|---|---|---|---|
| List | `list_swarm_defs` | `swarm_def_list` | `project_root` |
| Get | `get_swarm_def` | `swarm_def_detail` | `project_root`, `name` |
| Save | `save_swarm_def` | `ok` | `project_root`, `name`, `worktree_count`, `worktree_template`, `roster`, `include_terminal`, `scope` |
| Delete | `delete_swarm_def` | `ok` | `project_root`, `name`, `scope` |
| Run | `run_swarm` | `run_swarm_result` or `run_swarm_partial` | `project_root`, `swarm_name`, `vars` |

`run_swarm` responds with `run_swarm_result` (all agents spawned) or `run_swarm_partial` (some agents spawned, includes `error_code` and `error_message`).

## Schedule CRUD

| Operation | Request type | Response type | Key fields |
|---|---|---|---|
| List | `list_schedules` | `schedule_list` | `project_root` |
| Get | `get_schedule` | `schedule_detail` | `project_root`, `name` |
| Save | `save_schedule` | `ok` | `project_root`, `name`, `enabled`, `recurrence`, `start_at`, `trigger`, `target`, `scope`, `root`, `agent_name` |
| Delete | `delete_schedule` | `ok` | `project_root`, `name`, `scope` |
| Enable | `enable_schedule` | `ok` | `project_root`, `name` |
| Disable | `disable_schedule` | `ok` | `project_root`, `name` |

## Trigger CRUD + gate evaluation

| Operation | Request type | Response type | Key fields |
|---|---|---|---|
| List | `list_triggers` | `trigger_list` | `project_root` |
| Get | `get_trigger` | `trigger_detail` | `project_root`, `name` |
| Save | `save_trigger` | `ok` | `project_root`, `name`, `description`, `on`, `sequence`, `variables`, `scope` |
| Delete | `delete_trigger` | `ok` | `project_root`, `name`, `scope` |
| Evaluate gate | `evaluate_gate` | `gate_result` | `event`, `project_root`, `worktree_path` |
| Assign | `assign_trigger` | `assign_trigger_result` | `project_root`, `agent_id`, `trigger_name` |

Gate evaluation timeouts: 60 seconds per command, 5 minutes total.

`assign_trigger` binds a trigger to an agent. Response includes `sequence_len` (number of actions in trigger).

## Daemon lifecycle

- **Auto-start**: CLI calls `ensure_daemon()`, finds binary, spawns detached, polls health with exponential backoff (up to 3s)
- **Standalone mode**: Writes PID file to `~/.pu/daemon.pid`
- **Managed mode**: `--managed` flag (for macOS app); exits when parent process dies
- **Signals**: SIGTERM and SIGINT trigger graceful shutdown

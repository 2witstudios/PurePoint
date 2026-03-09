# IPC API Reference

The PurePoint daemon communicates with clients (CLI, macOS app) via NDJSON over a Unix domain socket.

## Transport

| Property | Value |
|---|---|
| Socket path | `~/.pu/daemon.sock` |
| Protocol version | 4 |
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
| `NOT_FOUND` | Template, agent def, swarm def, schedule, or trigger not found |
| `IO_ERROR` | File I/O failure |
| `INTERNAL_ERROR` | Unexpected internal error |
| `SPAWN_FAILED` | Agent spawn failed |

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
  "no_trigger": false
}
```

**Response**:
```json
{
  "type": "spawn_result",
  "worktree_id": "wt-abc12345",
  "agent_id": "ag-xyz98765",
  "status": "streaming"
}
```

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

KillTarget variants: `{"type": "agent", "id": "..."}`, `{"type": "worktree", "id": "..."}`, `{"type": "all", "include_root": false}`

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
| `split` | `leaf_id`, `axis` | Split a pane (axis: `"vertical"` or `"horizontal"`) |
| `close` | `leaf_id` | Close a pane |
| `focus` | `leaf_id`, `direction` | Move focus (direction: `"up"`, `"down"`, `"left"`, `"right"`) |
| `set_agent` | `leaf_id`, `agent_id` | Assign an agent to a pane |
| `get_layout` | (none) | Get current layout |

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

| Operation | Request type | Key fields |
|---|---|---|
| List | `list_templates` | `project_root` |
| Get | `get_template` | `project_root`, `name` |
| Save | `save_template` | `project_root`, `name`, `description`, `agent`, `body`, `scope` |
| Delete | `delete_template` | `project_root`, `name`, `scope` |

## Agent def CRUD

| Operation | Request type | Key fields |
|---|---|---|
| List | `list_agent_defs` | `project_root` |
| Get | `get_agent_def` | `project_root`, `name` |
| Save | `save_agent_def` | `project_root`, `name`, `agent_type`, `template`, `inline_prompt`, `tags`, `scope` |
| Delete | `delete_agent_def` | `project_root`, `name`, `scope` |

## Swarm def CRUD + execution

| Operation | Request type | Key fields |
|---|---|---|
| List | `list_swarm_defs` | `project_root` |
| Get | `get_swarm_def` | `project_root`, `name` |
| Save | `save_swarm_def` | `project_root`, `name`, `worktree_count`, `roster`, `scope` |
| Delete | `delete_swarm_def` | `project_root`, `name`, `scope` |
| Run | `run_swarm` | `project_root`, `swarm_name`, `vars` |

`run_swarm` responds with `run_swarm_result` (all agents spawned) or `run_swarm_partial` (some agents spawned, includes `error_code` and `error_message`).

## Schedule CRUD

| Operation | Request type | Key fields |
|---|---|---|
| List | `list_schedules` | `project_root` |
| Get | `get_schedule` | `project_root`, `name` |
| Save | `save_schedule` | `project_root`, `name`, `enabled`, `recurrence`, `start_at`, `trigger`, `scope` |
| Delete | `delete_schedule` | `project_root`, `name`, `scope` |
| Enable | `enable_schedule` | `project_root`, `name` |
| Disable | `disable_schedule` | `project_root`, `name` |

## Trigger CRUD + gate evaluation

| Operation | Request type | Key fields |
|---|---|---|
| List | `list_triggers` | `project_root` |
| Get | `get_trigger` | `project_root`, `name` |
| Save | `save_trigger` | `project_root`, `name`, `on`, `sequence`, `scope` |
| Delete | `delete_trigger` | `project_root`, `name`, `scope` |
| Evaluate gate | `evaluate_gate` | `event`, `project_root`, `worktree_path` |

Gate evaluation timeouts: 60 seconds per command, 5 minutes total.

## Daemon lifecycle

- **Auto-start**: CLI calls `ensure_daemon()`, finds binary, spawns detached, polls health with exponential backoff (up to 3s)
- **Standalone mode**: Writes PID file to `~/.pu/daemon.pid`
- **Managed mode**: `--managed` flag (for macOS app); exits when parent process dies
- **Signals**: SIGTERM and SIGINT trigger graceful shutdown

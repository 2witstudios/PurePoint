# Manifest Schema

The manifest at `.pu/manifest.json` is the single source of truth for workspace state. It is managed by the daemon; do not edit it directly.

All fields use camelCase JSON serialization. Optional fields are omitted when null or false.

## Root object

```json
{
  "version": 1,
  "projectRoot": "/path/to/project",
  "worktrees": {},
  "agents": {},
  "createdAt": "2026-03-08T00:00:00Z",
  "updatedAt": "2026-03-08T12:00:00Z"
}
```

| Field | Type | Description |
|---|---|---|
| `version` | number | Schema version (currently `1`) |
| `projectRoot` | string | Absolute path to the project |
| `worktrees` | map&lt;string, WorktreeEntry&gt; | Worktrees keyed by worktree ID |
| `agents` | map&lt;string, AgentEntry&gt; | Root-level agents keyed by agent ID |
| `createdAt` | datetime | When the workspace was initialized |
| `updatedAt` | datetime | Last modification timestamp |

## WorktreeEntry

| Field | Type | Required | Description |
|---|---|---|---|
| `id` | string | yes | Format: `wt-{8chars}` |
| `name` | string | yes | Human-readable name (git branch slug) |
| `path` | string | yes | Absolute path to worktree directory |
| `branch` | string | yes | Git branch name, typically `pu/{name}` |
| `baseBranch` | string | no | Base branch (e.g., `"main"`) |
| `status` | WorktreeStatus | yes | Lifecycle state |
| `agents` | map&lt;string, AgentEntry&gt; | yes | Agents in this worktree |
| `createdAt` | datetime | yes | ISO 8601 |
| `mergedAt` | datetime | no | When the worktree was merged |

### WorktreeStatus

| Value | Description |
|---|---|
| `active` | Worktree is in use |
| `merging` | Merge in progress |
| `merged` | Successfully merged |
| `failed` | Merge or operation failed |
| `cleaned` | Worktree removed and cleaned up |

## AgentEntry

| Field | Type | Required | Description |
|---|---|---|---|
| `id` | string | yes | Format: `ag-{8chars}` |
| `name` | string | yes | Human-readable name |
| `agentType` | string | yes | `claude`, `codex`, `opencode`, or `terminal` |
| `status` | AgentStatus | yes | Current state |
| `prompt` | string | no | Initial prompt text |
| `startedAt` | datetime | yes | When the agent was spawned |
| `completedAt` | datetime | no | When the agent exited |
| `exitCode` | number | no | Process exit code |
| `error` | string | no | Error message if failed |
| `pid` | number | no | OS process ID |
| `sessionId` | string | no | UUID v4 session identifier (claude agents) |
| `suspended` | bool | no | Whether the agent is suspended |
| `suspendedAt` | datetime | no | When suspended |
| `command` | string | no | Shell command used to spawn |
| `planMode` | bool | no | Whether spawned in plan/architect mode |
| `triggerName` | string | no | Bound trigger definition name |
| `triggerSeqIndex` | number | no | Current position in trigger sequence |
| `triggerState` | TriggerState | no | Trigger execution state |
| `triggerTotal` | number | no | Total actions in trigger sequence |
| `gateAttempts` | number | no | Gate retry count |
| `noTrigger` | bool | no | Skip auto-triggers for this agent |

### AgentStatus

| Value | Description |
|---|---|
| `streaming` | Active, producing output |
| `waiting` | Alive but idle, or suspended |
| `broken` | Process exited |

### TriggerState

| Value | Description |
|---|---|
| `active` | Trigger sequence in progress |
| `gating` | Evaluating a gate command |
| `completed` | All actions executed successfully |
| `failed` | Sequence stopped due to failure |

## Persistence

- **Writes**: Atomic via temp file, fsync, rename
- **Locking**: Advisory exclusive lock via `fs4` during updates
- **Watching**: macOS app uses GCD `DispatchSource` with 50ms debounce

## Backward Compatibility

- **`suspended` field**: Inferred as `true` when `suspendedAt` is present (supports manifests created before the `suspended` field was added)
- **Old status values**: Automatically mapped to new values during deserialization:
  - `spawning`, `running` → `streaming`
  - `idle`, `suspended` → `waiting`
  - `completed`, `failed`, `killed`, `lost` → `broken`

## Example

```json
{
  "version": 1,
  "projectRoot": "/Users/jono/myproject",
  "worktrees": {
    "wt-abc12345": {
      "id": "wt-abc12345",
      "name": "fix-auth",
      "path": "/Users/jono/myproject/.pu/worktrees/wt-abc12345",
      "branch": "pu/fix-auth",
      "baseBranch": "main",
      "status": "active",
      "agents": {
        "ag-xyz98765": {
          "id": "ag-xyz98765",
          "name": "Claude",
          "agentType": "claude",
          "status": "streaming",
          "prompt": "Fix the authentication bug in login.ts",
          "startedAt": "2026-03-08T12:05:00Z",
          "pid": 12345,
          "sessionId": "550e8400-e29b-41d4-a716-446655440000"
        }
      },
      "createdAt": "2026-03-08T12:00:00Z"
    }
  },
  "agents": {
    "ag-root1111": {
      "id": "ag-root1111",
      "name": "Researcher",
      "agentType": "claude",
      "status": "waiting",
      "prompt": "Research the codebase structure",
      "startedAt": "2026-03-08T11:00:00Z",
      "sessionId": "a1b2c3d4-e5f6-47a8-b9c0-d1e2f3a4b5c6",
      "planMode": true
    }
  },
  "createdAt": "2026-03-01T00:00:00Z",
  "updatedAt": "2026-03-08T12:05:30Z"
}
```

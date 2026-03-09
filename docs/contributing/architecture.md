# Architecture

PurePoint has three components: a CLI, a daemon, and a macOS app. They communicate over a Unix socket using NDJSON.

## System diagram

```
┌──────────┐     NDJSON/Unix Socket     ┌──────────────┐
│  pu CLI  │ <────────────────────────> │  pu-engine   │
└──────────┘                            │  (daemon)    │
                                        │              │
┌──────────┐     NDJSON/Unix Socket     │  PTY Host    │
│  macOS   │ <────────────────────────> │  Sessions    │
│  App     │                            │  Scheduler   │
└──────────┘                            │  Reaper      │
                                        └──────────────┘
```

- The CLI auto-starts the daemon if not running
- The macOS app launches the daemon in `--managed` mode (exits when parent dies)
- Protocol version handshake via `Health` request
- See [IPC API Reference](../reference/ipc-api.md) for the full protocol

## Rust crate structure

```
crates/
├── pu-core/     # Shared types library (no async deps)
├── pu-engine/   # Daemon binary (async, tokio)
└── pu-cli/      # CLI binary (thin client)
```

### Dependency graph

```
pu-cli ──> pu-core
pu-engine ──> pu-core

pu-cli --dev--> pu-engine  (integration tests)
```

### pu-core

Pure data types, serialization, and file I/O. No async runtime.

| Module | Responsibility |
|---|---|
| `types.rs` | Manifest, AgentEntry, WorktreeEntry, Config, AgentStatus |
| `protocol.rs` | IPC Request/Response enums (49 variants), protocol version |
| `manifest.rs` | Atomic read/write/update with file locking |
| `config.rs` | Config loading from `.pu/config.yaml` |
| `paths.rs` | All filesystem path conventions |
| `template.rs` | Prompt template parsing, rendering, CRUD |
| `agent_def.rs` | Agent definition YAML format, CRUD |
| `swarm_def.rs` | Swarm composition format, CRUD |
| `schedule_def.rs` | Schedule definitions, recurrence calculation |
| `trigger_def.rs` | Trigger definitions, event types, gate defs |
| `id.rs` | ID generation (wt-/ag- prefixed, UUID sessions) |
| `validation.rs` | Resource name validation (path traversal protection) |
| `error.rs` | PuError enum with error codes |

### pu-engine

Async daemon. Depends on pu-core + tokio + nix + tracing.

| Module | Responsibility |
|---|---|
| `main.rs` | Entry point, arg parsing, server loop |
| `engine.rs` | Core business logic, request dispatch, state management |
| `ipc_server.rs` | Unix socket server, NDJSON framing, connection handling |
| `pty_manager.rs` | PTY spawn via fork/setsid/execvp, process monitoring |
| `output_buffer.rs` | 4MB circular buffer with idle detection |
| `agent_monitor.rs` | Effective status computation |
| `attach_handler.rs` | APC escape parsing for terminal resize |
| `git.rs` | Worktree creation, hook installation |
| `gate.rs` | Gate evaluation with timeouts and retries |
| `daemon_lifecycle.rs` | PID file management, cleanup |

### pu-cli

Thin client. Depends on pu-core + clap + tokio.

| Module | Responsibility |
|---|---|
| `main.rs` | Clap CLI definitions (21 commands) |
| `client.rs` | NDJSON-over-Unix-socket request sender |
| `daemon_ctrl.rs` | Daemon auto-start with health polling |
| `commands/` | One module per CLI command |
| `output.rs` | Human-readable and JSON output formatting |
| `skill.rs` | Claude Code skill auto-installation |

## macOS app

SwiftUI + AppKit application.

| Layer | Key files | Responsibility |
|---|---|---|
| Entry | `purepoint_macosApp.swift` | App lifecycle, AppState creation |
| State | `AppState`, `SidebarState`, `GridState` | Observable state management |
| Models | `ManifestModel`, `AgentModel`, `WorkspaceModel` | Data models (Codable for manifest) |
| Services | `WorkspaceService`, `DaemonClient`, `CLIInstaller` | Daemon communication, CLI installation |
| Views | `Views/` (97 files) | SwiftUI views organized by feature |
| Terminal | `TerminalViewCache`, `ScrollableTerminal` | SwiftTerm integration with LRU caching |

Communication: The app uses `DaemonClient` (NDJSON over Unix socket) mirroring the Rust protocol types.

## Key patterns

### Manifest as source of truth

`.pu/manifest.json` is read/written by both Rust and Swift. Atomic writes (temp + rename), file locking (fs4), camelCase JSON keys for Swift Codable compatibility.

### Three-state agent model

Agents have exactly 3 states: `streaming`, `waiting`, `broken`. Computed from PTY output patterns and process state. Backward-compatible deserialization from legacy 8-state model.

### Local/global scope

Templates, agent defs, swarm defs, schedules, and triggers all follow the same pattern: `.pu/{type}/` (local) and `~/.pu/{type}/` (global). Local shadows global. See [Concepts: Scope](../guide/concepts.md#scope).

### Engine state

The `Engine` struct holds all runtime state:
- `sessions`: HashMap of agent handles (pid, master_fd, output_buffer)
- `grid_channels` / `status_channels`: Broadcast channels per project
- `registered_projects`: Projects the scheduler monitors
- Background tasks: session reaper (30s), scheduler (30s)

## Data flows

### Spawn flow

1. CLI sends `Spawn` request to daemon
2. Engine loads config, resolves agent type and launch args
3. Engine creates worktree (if needed) via `git worktree add`
4. PTY Manager forks, calls `setsid` + `execvp` to start agent
5. Engine injects prompt via stdin (claude/terminal) or CLI arg (codex/opencode)
6. Manifest updated with new worktree and agent entries
7. macOS app's ManifestWatcher picks up the change

### Output flow

1. Agent writes to PTY
2. OutputBuffer captures bytes in 4MB circular buffer
3. Attach stream sends hex-encoded chunks to connected clients
4. Logs request reads last N bytes from buffer
5. Idle detection: shell prompt patterns or 30s silence

### Trigger flow

1. Event fires (agent_idle detected, git hook calls `pu gate`)
2. Engine loads matching trigger defs
3. For each action: evaluate gate (if present), inject text (if present)
4. Gate retries on failure, sequence stops on final failure
5. Trigger state tracked in agent's manifest entry

# IPC & API

**Maturity: CONVERGING**

## Context

All PurePoint clients (CLI, desktop app, future mobile/web) need a structured way to communicate with the daemon. The API must support both simple request/response patterns (spawn an agent, kill a process) and real-time streaming (live agent output, state change notifications). Local clients need low-latency access; remote clients need network transport.

## Decisions

! [IPC-001] Newline-delimited JSON over Unix domain socket (`~/.pu/daemon.sock`) — simplest option that works, no code generation or schema compilation needed, debuggable with standard tools. 1MB max message size (`MAX_MESSAGE_SIZE`), 64-connection semaphore (`MAX_CONNECTIONS`). Stale socket file removed before bind. Tagged union encoding: `#[serde(tag = "type", rename_all = "snake_case")]` on Request/Response enums. Binary data (PTY I/O) hex-encoded within JSON. Implemented in `pu-engine/src/ipc_server.rs` (server), `pu-cli/src/client.rs` (client), `pu-core/src/protocol.rs` (types).

! [IPC-002] Daemon captures PTY output into a 1MB circular buffer per agent (`OutputBuffer` in `pu-engine/src/output_buffer.rs`). Reader task runs in `spawn_blocking`, reading 4096-byte chunks from the PTY master fd. Clients retrieve output via `Request::Logs { agent_id, tail }` for the last N bytes, or `Request::Attach { agent_id }` for interactive mode (returns buffered bytes, then streams live output; `Request::Input` sends keystrokes back). No direct client-to-agent connection needed.

! [IPC-005] Synchronous request-response — each request gets exactly one response on the same connection. `Spawn` returns immediately with agent/worktree IDs and initial status; clients poll via `Status` to track progress. Multiple request-response cycles supported per connection (loop until EOF). Long-running ops never block the connection. Implemented in `pu-engine/src/ipc_server.rs` (connection handler loop).

## Open Questions

? [IPC-003] Should the API be split into tiers or kept as one service?
Simple operations (spawn, kill, status) vs complex streaming (live output, state changes). Currently a single service handling both patterns. Separate services could run with different configurations. One service is simpler but mixes patterns. (Current implementation is single-service; question still valid for scaling.)

? [IPC-004] What authentication model for remote connections?
Local connections are authenticated by OS file permissions on the Unix socket. Remote connections need auth. Options: certificate-based, bearer token, API key. How lightweight can this be for a local-first tool? (Not yet implemented — local-only for now.)

## Design Directions

- Local transport: Unix domain socket (implemented)
- Remote transport: future consideration (see IPC-004)
- Real-time streaming via attach mode (PTY bytes forwarded to client)
- Concurrent client support via semaphore-limited connection pool
- Backwards-compatible API once stabilized (protocol version in health report)

## Research Notes

**Protocol details (from `pu-core/src/protocol.rs`):** `PROTOCOL_VERSION = 1`. Request variants: `Health`, `Init`, `Spawn`, `Status`, `Kill`, `Suspend`, `Resume`, `Logs`, `Attach`, `Input`, `Resize`, `SubscribeGrid`, `SubscribeStatus`, `GridCommand`, `Rename`, `DeleteWorktree`, `Shutdown`. Response variants: `HealthReport`, `InitResult`, `SpawnResult`, `StatusReport`, `AgentStatus`, `KillResult`, `SuspendResult`, `ResumeResult`, `LogsResult`, `AttachReady`, `Output`, `GridSubscribed`, `GridLayout`, `GridEvent`, `StatusSubscribed`, `StatusEvent`, `RenameResult`, `DeleteWorktreeResult`, `Ok`, `ShuttingDown`, `Error`. `KillTarget` enum supports `Agent(id)`, `Worktree(id)`, or `All`. `SuspendTarget` enum supports `Agent(id)` or `All`. `GridCommand` enum supports `Split`, `Close`, `Focus`, `SetAgent`, `GetLayout`.

**Error handling:** Parse errors return `Response::Error { code: "PARSE_ERROR", message }` without closing the connection, allowing retry on the same connection.

**Client timeout:** CLI uses a 30-second request timeout (`pu-cli/src/client.rs`).

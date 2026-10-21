# IPC & API

**Maturity: CONVERGING**

## Context

All PurePoint clients (CLI, desktop app, future mobile/web) need a structured way to communicate with the daemon. The API must support both simple request/response patterns (spawn an agent, kill a process) and real-time streaming (live agent output, state change notifications). Local clients need low-latency access; remote clients need network transport.

## Decisions

! [IPC-001] Newline-delimited JSON over Unix domain socket (`~/.pu/daemon.sock`) — simplest option that works, no code generation or schema compilation needed, debuggable with standard tools. 1MB max message size (`MAX_MESSAGE_SIZE`), 1024-connection semaphore (`MAX_CONNECTIONS`) — the app holds a status and a grid stream per project plus an attach stream per pane, so 64 was exhausted in practice; connections over the limit get one `BUSY` error line and are closed, and the accept loop never waits for a permit. Stale socket file removed before bind, which is safe only because the daemon holds `daemon.sock.lock`. Tagged union encoding: `#[serde(tag = "type", rename_all = "snake_case")]` on Request/Response enums. Binary data (PTY I/O) hex-encoded within JSON. Implemented in `pu-engine/src/ipc_server/mod.rs` (server), `pu-cli/src/client.rs` (client), `pu-core/src/protocol/mod.rs` (types).

! [IPC-002] Daemon captures PTY output into a 4MB circular buffer per agent (`OutputBuffer` in `pu-engine/src/output_buffer.rs`). Reader task runs in `spawn_blocking`, reading 4096-byte chunks from the PTY master fd. Clients retrieve output via `Request::Logs { agent_id, tail }` for the last N bytes, or `Request::Attach { agent_id }` for interactive mode (returns buffered bytes, then streams live output; `Request::Input` sends keystrokes back). No direct client-to-agent connection needed.

! [IPC-005] Synchronous request-response — each request gets exactly one response on the same connection. `Spawn` returns immediately with agent/worktree IDs and initial status; clients poll via `Status` to track progress. Multiple request-response cycles supported per connection (loop until EOF). Long-running ops never block the connection. Implemented in `pu-engine/src/ipc_server.rs` (connection handler loop).

! [IPC-004] Remote connections are authenticated by SSH, not by the daemon. A remote machine's daemon is reached through a system `ssh` Unix-socket forward to its `~/.pu/daemon.sock`; sshd opens that socket as the authenticated SSH user, so SSH authentication plus the remote socket's file permissions are the authorization boundary and the protocol itself carries no credentials (see [MACH-001, MACH-002](../product/machines.md)). Local connections remain authenticated by the socket's file permissions.

## Open Questions

? [IPC-003] Should the API be split into tiers or kept as one service?
Simple operations (spawn, kill, status) vs complex streaming (live output, state changes). Currently a single service handling both patterns. Separate services could run with different configurations. One service is simpler but mixes patterns. (Current implementation is single-service; question still valid for scaling.)

## Design Directions

- Local transport: Unix domain socket (implemented)
- Remote transport: SSH Unix-socket forwarding of the same protocol (IPC-004, [Machines over SSH](../product/machines.md)); no network listener in the daemon
- Real-time streaming via attach mode (PTY bytes forwarded to client)
- Concurrent client support via semaphore-limited connection pool
- Backwards-compatible API once stabilized (protocol version in health report)

## Research Notes

**Protocol details (from `pu-core/src/protocol/mod.rs`):** `PROTOCOL_VERSION = 11`. Request variants: `Read`, `ChannelRead`, `ChannelSend`, `ChannelEdit`, `ChannelReact`, `Inventory`, `ResolveAgent`, `Health`, `Init`, `Spawn`, `Status`, `Kill`, `Suspend`, `Resume`, `Logs`, `SpawnShell`, `Attach`, `Input`, `Resize`, `SubscribeGrid`, `SubscribeStatus`, `GridCommand`, `Rename`, `AssignTrigger`, `CreateWorktree`, `DeleteWorktree`, `ListTemplates`, `GetTemplate`, `SaveTemplate`, `DeleteTemplate`, `ListAgentDefs`, `GetAgentDef`, `SaveAgentDef`, `DeleteAgentDef`, `ListSwarmDefs`, `GetSwarmDef`, `SaveSwarmDef`, `DeleteSwarmDef`, `RunSwarm`, `ListSchedules`, `GetSchedule`, `SaveSchedule`, `DeleteSchedule`, `EnableSchedule`, `DisableSchedule`, `GetGlobalAgentSettings`, `UpdateGlobalAgentSettings`, `GetConfig`, `UpdateAgentConfig`, `ListTriggers`, `GetTrigger`, `SaveTrigger`, `DeleteTrigger`, `EvaluateGate`, `Shutdown`, `Diff`, `Pulse`, `FsList`, `FsStat`, `FsRead`, `FsWrite`, `FsDelete`, `SubscribeFs`, `GitExec`, `AttachFile`. Response variants: `ChannelHistory`, `ChannelMessage`, `Inventory`, `AgentResolved`, `HealthReport`, `InitResult`, `SpawnResult`, `StatusReport`, `AgentStatus`, `KillResult`, `SuspendResult`, `ResumeResult`, `LogsResult`, `ReadResult`, `AttachReady`, `Output`, `GridSubscribed`, `GridLayout`, `GridEvent`, `StatusSubscribed`, `StatusEvent`, `RenameResult`, `AssignTriggerResult`, `CreateWorktreeResult`, `DeleteWorktreeResult`, `TemplateList`, `TemplateDetail`, `AgentDefList`, `AgentDefDetail`, `SwarmDefList`, `SwarmDefDetail`, `RunSwarmResult`, `RunSwarmPartial`, `ScheduleList`, `ScheduleDetail`, `GlobalAgentSettingsReport`, `ConfigReport`, `TriggerList`, `TriggerDetail`, `GateResult`, `DiffResult`, `PulseReport`, `FsListResult`, `FsStatResult`, `FsReadResult`, `FsWriteResult`, `FsSubscribed`, `FsEvent`, `GitExecResult`, `AttachFileResult`, `Ok`, `ShuttingDown`, `Error`. `KillTarget` enum supports `Agent(id)`, `Worktree(id)`, `All`, or `AllWorktrees`. `SuspendTarget` enum supports `Agent(id)` or `All`. `GridCommand` enum supports `Split`, `Close`, `Focus`, `SetAgent` (optional `leaf_id`; targets the pane's active tab), `NewTab`, `SelectTab`, `CloseTab`, `MoveTab`, `BreakTab`, `GetLayout`.

**Error handling:** Parse errors return `Response::Error { code: "PARSE_ERROR", message }` without closing the connection, allowing retry on the same connection.

**Client timeout:** CLI uses a 30-second request timeout (`pu-cli/src/client.rs`).

**Protocol v7 compatibility:** Adds `Inventory`/`ResolveAgent` requests and `Inventory`/`AgentResolved` responses. CLI command discovery requires an exact health-report version match. An incompatible daemon is rejected with update/restart guidance, preserving active sessions; a BUSY response does not establish compatibility and is returned without launching another daemon.


Conversation reads (protocol 8): `Read { project_root, agent_id, limit, since?, session_id? }` returns `ReadResult` with runtime, exact session ID, source kind, normalized records, cursor and completeness indicators. See [Agent Conversation Reads](../product/agent-read.md). Resume uses exact recorded native identity; legacy unbound workers require `pu read AGENT --session ID`. No newest-session selection.

File and git access (protocol 9): `FsList`, `FsStat`, `FsRead`, `FsWrite`, `FsDelete`, `SubscribeFs` (streams coalesced `FsEvent { root, paths }`, ~200 ms) and `GitExec` (`git` or `gh` only, run without a shell, 8 MiB cap per output stream). Every path is canonicalized and must lie inside a registered project root, one of its worktrees, or an agent transcript directory under the daemon user's home; otherwise `Error { code: "path_not_allowed" }`. A write whose `expected_modified_ms` no longer matches returns `Error { code: "conflict" }`. Binary payloads (`data_base64`, `stdin_base64`, `stdout_base64`, `stderr_base64`) are standard base64. Field shapes and defaults: [Machines over SSH](../product/machines.md#daemon-file-and-git-requests-protocol-version-9). Implemented in `pu-engine/src/engine/fs_ops.rs` and `pu-engine/src/ipc_server/streams.rs`.

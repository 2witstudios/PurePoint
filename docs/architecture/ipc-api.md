# IPC & API

**Maturity: SEED**

## Context

All PurePoint clients (CLI, macOS app, future iOS/web) need a structured way to communicate with the daemon. The API must support both simple request/response patterns (spawn an agent, kill a process) and real-time streaming (live agent output, state change notifications). Local clients need low-latency access via Unix socket; remote clients need TCP. A leading option is gRPC with tonic, split into two service tiers: Tier 1 for agent/conductor operations (simple request/response) and Tier 2 for dashboard/rich clients (streaming RPCs).

## Open Questions

? [IPC-001] Is gRPC the right choice, or should we consider alternatives?
gRPC with tonic gives us: typed schemas, streaming, code generation for Swift/Rust. Alternatives: Cap'n Proto (zero-copy, but less tooling), JSON-RPC over Unix socket (simpler but no streaming), custom binary protocol. gRPC-Swift exists but is in transition (grpc-swift-nio vs grpc-swift-protobuf). How mature is the Swift gRPC client?

? [IPC-002] How should terminal output streaming work through the daemon?
Agents run in tmux managed by the daemon. How does raw terminal output get from tmux to daemon to client? Options: daemon captures pane output and streams via gRPC, client attaches directly to tmux (bypassing daemon), or hybrid where daemon proxies PTY output.

? [IPC-003] Should the two-tier split be two gRPC services or one?
Two separate service definitions (AgentService + DashboardService) vs one service with logical grouping. Two services could run on different ports/sockets with different auth. One service is simpler but mixes simple RPCs with complex streaming.

? [IPC-004] What authentication model for TCP connections?
Unix socket connections are authenticated by file permissions. TCP connections (remote clients) need auth. Options: mTLS (certificate-based), bearer token (daemon generates a secret on start), API key in metadata. How lightweight can we keep this for a local-first tool?

? [IPC-005] How should long-running operations be handled?
Some operations take time (merge with conflicts, large spawn). Should these be synchronous RPCs that block, async operations that return an operation ID for polling, or server-streaming RPCs that push progress updates? The Wait RPC already implies some async pattern.

## Design Directions

- Unix socket (local) and TCP (remote) transports
- Server-side streaming for real-time updates
- Client code generation for both Rust (CLI) and Swift (macOS app)
- Concurrent client support without deadlocks
- Backwards-compatible API once stabilized (proto versioning)

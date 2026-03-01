# IPC & API

**Maturity: SEED**

## Context

All PurePoint clients (CLI, desktop app, future mobile/web) need a structured way to communicate with the daemon. The API must support both simple request/response patterns (spawn an agent, kill a process) and real-time streaming (live agent output, state change notifications). Local clients need low-latency access; remote clients need network transport.

## Open Questions

? [IPC-001] What IPC/API protocol should PurePoint use?
Options include gRPC, Cap'n Proto, JSON-RPC over Unix socket, REST/HTTP, custom binary protocol. Trade-offs: typed schemas, streaming support, code generation for multiple languages, ecosystem maturity, complexity.

? [IPC-002] How should terminal output streaming work through the daemon?
Agents produce continuous output. How does raw terminal output get from the agent process to the daemon to the client? Options: daemon captures output and streams via API, client connects directly to agent process, or a hybrid.

? [IPC-003] Should the API be split into tiers or kept as one service?
Simple operations (spawn, kill, status) vs complex streaming (live output, state changes). Separate services could run with different configurations. One service is simpler but mixes patterns.

? [IPC-004] What authentication model for remote connections?
Local connections can be authenticated by OS file permissions. Remote connections need auth. Options: certificate-based, bearer token, API key. How lightweight can this be for a local-first tool?

? [IPC-005] How should long-running operations be handled?
Some operations take time (merge with conflicts, large spawn). Should these be synchronous, async with polling, or streaming with progress updates?

## Design Directions

- Local and remote transport support
- Real-time streaming for live updates
- Client code generation for multiple languages
- Concurrent client support
- Backwards-compatible API once stabilized

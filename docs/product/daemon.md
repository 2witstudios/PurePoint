# Daemon

**Maturity: CONVERGING** | ID Prefix: DMN | Dependencies: `architecture/daemon-engine.md`, `architecture/ipc-api.md`

## Purpose

The long-running background process that owns all state and operations. Manages agent processes, worktrees, scheduling, and serves the API. Single source of truth for the entire system.

## Conceptual Model

```
Daemon (pu-engine binary)
  Engine (core state: projects, agents, worktrees, manifest I/O)
  IPC Server (Unix socket listener, connection pool, request routing)
  PTY Manager (native PTY host: fork/setsid/execvp, master fd ownership)
  Agent Monitor (effective_status: exit code + prompt detection + idle timeout)
  Output Buffers (1MB circular buffer per agent)
  Git Integration (worktree create/remove via git commands)
  Daemon Lifecycle (PID file, socket cleanup, signal handling, shutdown)
```

## Research Notes

**Single daemon, per-project state keyed by project root.** The daemon is a single process serving all projects. Each request includes a `project_root` parameter to scope operations. `Engine` maintains per-project state internally. `Request::Init { project_root }` registers a project; subsequent `Spawn`/`Status`/`Kill` requests reference it. `Response::HealthReport` includes a `projects: Vec<String>` listing all registered project roots and `agent_count: usize` across all projects.

**Global inventory (implemented 2026-10-10):** Production startup loads canonical project roots from `<socket>.projects.json`; successful Init and validated project use register roots atomically. Alternate sockets have independent registries. Invalid registrations do not enter the set, unavailable registered projects remain visible with errors, and restart does not spawn agents. A derived Inventory query combines manifests with live ownership metadata; no duplicate global agent database is stored. Global summary and flat agent/worktree records share count semantics, and Health now returns known projects and the count of live session handles rather than all retained handles. In-memory Engine::new remains available for isolated embedding/tests.

**Daemon binary:** `pu-engine` (separate from `pu-cli`). Both modes take an exclusive lock on `daemon.sock.lock` (one daemon per socket) and write the PID file; managed mode (`--managed`) additionally exits when its parent app dies. Socket path configurable via `--socket <path>`, defaults to `~/.pu/daemon.sock`.

### [DMN-001] Project scope alternatives

Research from `engine/inventory.rs`, `engine/mod.rs` and the socket-specific project registry supports one multi-project daemon: local clients share process ownership, capacity and discovery. A daemon per project isolates failures but duplicates lifecycle and connection state and makes global ownership routing a client concern. Retain the existing single daemon with canonical per-project state and explicit request scope; independent sockets remain an intentional deployment boundary.

### [DMN-002] Protocol mismatch alternatives

`pu-cli/src/daemon_ctrl.rs` and `docs/architecture/ipc-api.md` establish exact health-version matching for command discovery. Reject mismatches with update/restart guidance while preserving running sessions. Automatic replacement would simplify an upgrade but can interrupt agent work and is rejected. Do not claim that a BUSY reply proves compatibility or launch another daemon in response to it.

### Local access prerequisite for LAN execution

The owner authorized execution of the reviewed LAN plan at PageSpace page `c93of1sevwu4kquan9qki721`. Inspection found `IpcServer::bind` unlinks the configured path and binds without explicit mode or peer checks, and startup creates a parent without enforcing its ownership/privacy. The socket lock prevents duplicate same-socket daemon instances, but does not itself establish file identity, directory ownership or access permissions. Validate the local boundary before introducing a remote principal. Unix permission checks protect against other OS users; same-account agent containment remains outside the trusted-fleet v1 model.

## LAN foundation requirements

- REQ-DMN-003: Given a daemon state/socket directory, should validate ownership and a private directory boundary, rejecting unsafe symlinks/types/owners without modifying unrelated filesystem entries.
- REQ-DMN-004: Given an existing socket path, should remove only a verified stale socket owned by the daemon user after exclusive socket-lock acquisition; should refuse regular files, symlinks and foreign entries.
- REQ-DMN-005: Given a newly bound Unix socket, should enforce owner-only access before serving and validate peer OS identity where supported.
- REQ-DMN-006: Given local lifecycle metadata, should use private, validated, durable state and preserve single-writer locking across both managed and standalone modes.
- REQ-DMN-007: Given test/build activity during LAN development, should use isolated scratch outputs and sockets and never overwrite installed daemon binaries or interrupt other active sessions.

## Open Questions

? [DMN-001] Should the daemon support multiple concurrent projects, or one daemon instance per project?
(Candidates and recommendation are recorded above; wider storage/recovery choices remain separate.)

? [DMN-002] How should the daemon handle version mismatches between CLI and daemon (e.g., after an update)?
(Exact mismatch rejection is implemented; the research above records its trade-off against automatic replacement.)

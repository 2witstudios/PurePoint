# Daemon

**Maturity: EXPLORING** | ID Prefix: DMN | Dependencies: `architecture/daemon-engine.md`, `architecture/ipc-api.md`

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

**Global inventory (implemented 2026-10-10):** Production startup loads canonical project roots from `<socket>.projects.json`; successful Init registers roots atomically (see explicit project membership below). Alternate sockets have independent registries. Invalid registrations do not enter the set, unavailable registered projects remain visible with errors, and restart does not spawn agents. A derived Inventory query combines manifests with live ownership metadata; no duplicate global agent database is stored. Global summary and flat agent/worktree records share count semantics, and Health now returns known projects and the count of live session handles rather than all retained handles. In-memory Engine::new remains available for isolated embedding/tests.

**Explicit project membership (implemented 2026-10-21, protocol 13):** The registry is each machine's one project list, and only `Init` adds to it and `RemoveProject` removes from it. Using a project (status, spawn, subscriptions) no longer registers it — that made a project removed on one device reappear as soon as another device still watching it sent a request. Removal keeps the project's files and `.pu/` so adding it again restores it; live agents block removal unless the caller asks to suspend them. `SubscribeProjects` pushes the full list on every change, and the macOS app's sidebar, each machine's project list and the phone bridge all follow the registry instead of keeping their own lists. Registered roots whose folder exists without a manifest (removed by clients that deleted `.pu/manifest.json`) are dropped by the next global inventory.

**Daemon binary:** `pu-engine` (separate from `pu-cli`). Both modes take an exclusive lock on `daemon.sock.lock` (one daemon per socket) and write the PID file; managed mode (`--managed`) additionally exits when its parent app dies. Socket path configurable via `--socket <path>`, defaults to `~/.pu/daemon.sock`.

## Open Questions

? [DMN-001] Should the daemon support multiple concurrent projects, or one daemon instance per project?
(Research note above shows current implementation is single-daemon multi-project, but the tradeoffs haven't been fully evaluated.)

? [DMN-002] How should the daemon handle version mismatches between CLI and daemon (e.g., after an update)?
(Protocol version is in HealthReport but no mismatch handling is implemented yet.)

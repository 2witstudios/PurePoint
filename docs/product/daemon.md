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

**Global inventory (implemented 2026-10-10):** Production startup loads canonical project roots from `<socket>.projects.json`; successful Init registers roots atomically (see explicit project membership below). Alternate sockets have independent registries. Invalid registrations do not enter the set, unavailable registered projects remain visible with errors, and restart does not spawn agents. A derived Inventory query combines manifests with live ownership metadata; no duplicate global agent database is stored. Global summary and flat agent/worktree records share count semantics, and Health now returns known projects and the count of live session handles rather than all retained handles. In-memory Engine::new remains available for isolated embedding/tests.

**Explicit project membership (implemented 2026-10-21, protocol 13):** The registry is each machine's one project list, and only `Init` adds to it and `RemoveProject` removes from it. Using a project (status, spawn, subscriptions) no longer registers it — that made a project removed on one device reappear as soon as another device still watching it sent a request. Removal keeps the project's files and `.pu/` so adding it again restores it; live agents block removal unless the caller asks to suspend them. `SubscribeProjects` pushes the full list on every change, and the macOS app's sidebar, each machine's project list and the phone bridge all follow the registry instead of keeping their own lists. Registered roots whose folder exists without a manifest (removed by clients that deleted `.pu/manifest.json`) are dropped in the background when the daemon starts; a manifest deleted while it runs leaves the project listed as unavailable until it is removed or the daemon restarts. Inventory never changes the registry.

**Project order (implemented 2026-10-22, protocol 14):** The registry is an ordered list and the single source of truth for a machine's project order. New projects are appended; removing one keeps the rest in order. `Request::ReorderProjects { project_roots }` (protocol 14) moves the listed roots to the front and keeps the rest in their current order. Inventory, Health and the `SubscribeProjects` stream report roots in registry order, and a reorder is pushed to every subscriber. The Mac sidebar (local and machine sections), the iPad and, through the bridge's `workspace.reorderProjects`, the phone all show this order and save reorders through it.

**Daemon binary:** `pu-engine` (separate from `pu-cli`). Both modes take an exclusive lock on `daemon.sock.lock` (one daemon per socket) and write the PID file; managed mode (`--managed`) additionally exits when its parent app dies. Socket path configurable via `--socket <path>`, defaults to `~/.pu/daemon.sock`.

## Open Questions

? [DMN-001] Should the daemon support multiple concurrent projects, or one daemon instance per project?
(Narrowed to two options below; the current implementation is Option A.)

? [DMN-002] How should the daemon handle version mismatches between CLI and daemon (e.g., after an update)?
(Narrowed to two options below. Clients now detect a mismatch and refuse to proceed, which is Option A.)

## Research Notes: Open Questions

#### [DMN-001] One daemon for all projects, or one per project?
**Researched: 2026-10-22**

The constraints come from what already ships:
- One daemon per socket, enforced by an exclusive `flock` on `daemon.sock.lock` (`architecture/daemon-engine.md`, Design Directions).
- Each socket has its own durable project registry (`<socket>.projects.json`, `pu-core/src/paths.rs`), which now also holds the project order.
- The global inventory Point Guard relies on was designed as a daemon-owned registry. DAEMON-002 chose this over clients aggregating per-project queries (`architecture/daemon-engine.md`, Research Notes).
- A machine is reached through one forwarded daemon socket (`product/machines.md`), and the phone bridge queries one socket per machine (`apps/purepoint-mobile/bridge/daemon.js`).

Option A: one daemon per socket serving every project, scoped by `project_root` on each request (current).
- Pro: one process to start, probe and upgrade per machine. Global inventory, agent ownership resolution and project order are single queries against one registry.
- Pro: remote machines, the phone bridge and the Mac sidebar all address a machine as one endpoint.
- Pro: isolation is still available when it's wanted: a separate `--socket` gets its own daemon, lock and registry.
- Con: a daemon crash or restart affects every project on that socket. Crash recovery (DAEMON-003) therefore matters more.
- Con: one event loop and one connection limit (`BUSY`) are shared by all projects.

Option B: one daemon instance per project.
- Pro: a fault or restart in one project's daemon can't touch another project's agents.
- Pro: each project could upgrade independently.
- Con: global views (Point Guard inventory, `pu projects`, the phone, a machine's project list and its order) need discovery and aggregation across N sockets. DAEMON-002 rejected that as Option A: observations at different times, ownership routing left to callers.
- Con: N processes, PID files and auto-starts per machine, and N sockets to forward for each remote machine.

Recommendation: Option A. Every cross-project feature built so far depends on one registry per machine. Per-project isolation is better served by Option A's separate sockets plus a stronger DAEMON-003 crash-recovery model than by splitting the daemon.

#### [DMN-002] Handling CLI/app and daemon version mismatches
**Researched: 2026-10-22**

What happens today:
- `Request::Health` reports `protocol_version`.
- The CLI probes health before every command. On a mismatch it refuses with a message to update `pu` and `pu-engine` together, and it never restarts a live daemon, which would kill its agents (`pu-cli/src/daemon_ctrl.rs`, `check_daemon_health`).
- A remote machine on another version is reported with "update PurePoint on both machines" (`ensure_remote_daemon`).
- The Mac app marks such a machine "Incompatible (protocol N)" and sends it no further requests (`MachineState.swift`, `MachineConnection.swift`).
- The phone bridge reports each machine's `protocolVersion` (`bridge/daemon.js`).
- Compatibility is strict equality on `PROTOCOL_VERSION`.

Option A: strict equality; refuse and explain, never auto-restart (current).
- Pro: simple. A client never sends a request shape the daemon can't parse, and agents are never killed by an automatic restart.
- Con: every protocol bump, including purely additive ones such as protocol 14's `reorder_projects`, forces every machine and client to upgrade in lockstep. Until then, remote machines show as incompatible.

Option B: compatibility ranges or capability flags. The daemon advertises a minimum supported version or a feature list, and clients hide features the daemon lacks. The phone bridge already does this with its `workspace` feature flags (`projectOrder`).
- Pro: additive operations stop breaking mixed-version fleets. An older remote machine stays usable without the new features.
- Con: clients need a test matrix across supported versions. Breaking shape changes still need a hard bump, and someone has to decide which changes are additive.

Recommendation: Option A until more than one machine per user is common; then Option B, with the bridge's feature flags as the pattern.


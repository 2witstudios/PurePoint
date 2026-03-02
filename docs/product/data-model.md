# Data Model

**Maturity: EXPLORING** | ID Prefix: DM | Dependencies: none (defines types used by all other domains)

## Purpose

Defines the core entities, relationships, and state machines that make up PurePoint's domain model. Everything flows from this: the storage schema, API messages, CLI output, and dashboard state.

## Conceptual Model

```
Project
  Sessions (units of work)
    Worktrees (isolated branches for parallel work)
      Agents (AI coding agents running in the worktree)
        Live output stream
        Events (spawned, prompt sent, tool used, completed, failed)
        Summaries (auto-generated)
        Result (final output/artifacts)
      Memory (per-worktree context)
    Session memory (decisions, outcomes, patterns)
  Project memory (cross-session knowledge)
```

## Research Notes

**Manifest v1 in Rust (`pu-core/src/types.rs`):** camelCase JSON for macOS app compatibility (`#[serde(rename_all = "camelCase")]`). Manifest version always `1` on `Manifest::new()`.

**Core types:**
```
Manifest:
  version: u32
  projectRoot: String
  worktrees: HashMap<String, WorktreeEntry>
  agents: HashMap<String, AgentEntry>  (root-level agents, no worktree)
  createdAt: DateTime<Utc>
  updatedAt: DateTime<Utc>

WorktreeEntry:
  id, name, path, branch: String
  baseBranch: Option<String>
  status: WorktreeStatus (Active, Merging, Merged, Failed, Cleaned)
  agents: HashMap<String, AgentEntry>
  createdAt: DateTime<Utc>
  mergedAt: Option<DateTime<Utc>>

AgentEntry:
  id, name, agentType: String
  status: AgentStatus (Spawning, Running, Idle, Completed, Failed, Killed, Lost)
  prompt: Option<String>
  startedAt: DateTime<Utc>
  completedAt, error, sessionId: Option<String>
  exitCode: Option<i32>
  pid: Option<u32>
```

**ID generation (`pu-core/src/id.rs`):** nanoid with custom alphabet `[a-z0-9]` (36 chars), length 8:
- Worktree: `wt-{nanoid8}` (11 chars total)
- Agent: `ag-{nanoid8}` (11 chars total)
- Session: `ses-{nanoid8}` (12 chars total)

**Atomic writes (`pu-core/src/manifest.rs`):** Write to temp file, `fsync`, then `rename` — prevents partial reads. Advisory locking via `fs4` crate (`FileExt` for flock).

**Agent lookup:** `Manifest::find_agent(id)` searches root agents first, then worktree agents. Returns `AgentLocation::Root(&AgentEntry)` or `AgentLocation::Worktree { worktree, agent }`. `Manifest::all_agents()` flattens root + all worktree agents into `Vec<&AgentEntry>`.

**Manifest shape (proven in ppg-cli):** The `.pu/manifest.json` file is the source of truth for workspace state. The Rust types mirror the Swift `ManifestModel` from the macOS app, with camelCase serialization for compatibility.

## Open Questions

? [DM-001] Should sessions be explicit user-created boundaries, or implicit based on time gaps in activity?

? [DM-002] How should the data model handle agent re-use across worktrees — new agent entry per worktree, or a single entry that moves?

? [DM-003] What agent types should be supported, and should the set be extensible?
(Current implementation: configurable via `.pu/config.yaml` with `AgentConfig` struct — `name`, `command`, `prompt_flag`, `interactive` flag. Default agent is "claude" with `command: "claude"`. The set is extensible via config.)

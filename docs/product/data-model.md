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
  status: AgentStatus (Streaming, Waiting, Broken)
  prompt: Option<String>
  startedAt: DateTime<Utc>
  completedAt: Option<DateTime<Utc>>
  exitCode: Option<i32>
  error: Option<String>
  pid: Option<u32>
  sessionId: Option<String>
  suspendedAt: Option<DateTime<Utc>>
  suspended: bool (default false, inferred true when suspendedAt present)
```

**ID generation (`pu-core/src/id.rs`):** nanoid with custom alphabet `[a-z0-9]` (36 chars), length 8:
- Worktree: `wt-{nanoid8}` (11 chars total)
- Agent: `ag-{nanoid8}` (11 chars total)
- Session: UUID v4 (e.g. `550e8400-e29b-41d4-a716-446655440000`)

**Atomic writes (`pu-core/src/manifest.rs`):** Write to temp file, `fsync`, then `rename` — prevents partial reads. Advisory locking via `fs4` crate (`FileExt` for flock).

**Agent lookup:** `Manifest::find_agent(id)` searches root agents first, then worktree agents. Returns `AgentLocation::Root(&AgentEntry)` or `AgentLocation::Worktree { worktree, agent }`. `Manifest::all_agents()` flattens root + all worktree agents into `Vec<&AgentEntry>`.

**Manifest shape (proven in the original TypeScript CLI):** The `.pu/manifest.json` file is the source of truth for workspace state. The Rust types mirror the Swift `ManifestModel` from the macOS app, with camelCase serialization for compatibility.

**Orchestration types (pu-core):**
```
AgentDefinition:
  name: String
  agentType: String (default: "claude")
  defaultPrompt: Option<String> (template name or inline)
  tags: Vec<String>
  scope: Scope (Local | Global)

SwarmDefinition:
  name: String
  roster: Vec<RosterEntry> (agent:role:qty)
  worktreeCount: u32
  worktreeTemplate: String
  includeTerminal: bool
  scope: Scope

SavedPrompt (template):
  name: String
  body: String
  description: String
  agent: String
  scope: Scope

ScheduleDef:
  name: String
  enabled: bool
  recurrence: String
  startAt: Option<DateTime<Utc>>
  trigger: ScheduleTrigger (AgentDef | SwarmDef | InlinePrompt)
  triggerName: Option<String>
  triggerPrompt: Option<String>
  agent: String
  variables: HashMap<String, String>
  projectRoot: Option<String>
  scope: Scope
```

**Config extension:** `default_agent_type` field on `.pu/config.yaml` — sets default agent type for spawning.

## Open Questions

? [DM-001] Should sessions be explicit user-created boundaries, or implicit based on time gaps in activity?

? [DM-002] How should the data model handle agent re-use across worktrees — new agent entry per worktree, or a single entry that moves?

? [DM-003] What agent types should be supported, and should the set be extensible?
(Current implementation: configurable via `.pu/config.yaml` with `AgentConfig` struct — `name`, `command`, `prompt_flag`, `interactive` flag. Default agent is "claude" with `command: "claude"`. The set is extensible via config.)

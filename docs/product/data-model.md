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
  worktrees: IndexMap<String, WorktreeEntry>  (insertion-order preserved)
  agents: IndexMap<String, AgentEntry>  (root-level agents, no worktree)
  createdAt: DateTime<Utc>
  updatedAt: DateTime<Utc>

WorktreeEntry:
  id, name, path, branch: String
  baseBranch: Option<String>
  status: WorktreeStatus (Active, Merging, Merged, Failed, Cleaned)
  agents: IndexMap<String, AgentEntry>  (insertion-order preserved)
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
  command: Option<String>  (terminal command, for terminal agents)
  planMode: bool (default false, read-only research mode)
  triggerName: Option<String>  (bound trigger definition name)
  triggerSeqIndex: Option<u32>  (current step in trigger sequence)
  triggerState: Option<TriggerState> (Active, Gating, Completed, Failed)
  triggerTotal: Option<u32>  (total steps in trigger sequence)
  gateAttempts: Option<u32>  (number of gate evaluation attempts)
  noTrigger: bool (default false, disable event triggers for this agent)
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
AgentDef:
  name: String
  agent_type: String (default: "claude")
  template: Option<String> (template name reference)
  inline_prompt: Option<String> (inline prompt text)
  tags: Vec<String>
  scope: String ("local" | "global")
  available_in_command_dialog: bool (default: true)
  icon: Option<String>
  command: Option<String> (for terminal agents)

SwarmDef:
  name: String
  worktree_count: u32 (default: 1)
  worktree_template: String (branch template with {index} placeholder)
  roster: Vec<SwarmRosterEntry>
  include_terminal: bool
  scope: String

SwarmRosterEntry:
  agent_def: String (reference to AgentDef name)
  role: String
  quantity: u32 (default: 1)

Template:
  name: String
  body: String
  description: String
  agent: String
  source: String ("local" | "global")
  command: Option<String> (for terminal templates)

ScheduleDef:
  name: String
  enabled: bool
  recurrence: Recurrence (None | Hourly | Daily | Weekdays | Weekly | Monthly)
  start_at: DateTime<Utc>
  next_run: Option<DateTime<Utc>>
  trigger: ScheduleTrigger
  project_root: String
  target: String
  root: bool (spawn in project root vs worktree)
  agent_name: Option<String> (worktree branch name when root=false)
  scope: String
  created_at: DateTime<Utc>

ScheduleTrigger (tagged enum):
  AgentDef { name: String }
  SwarmDef { name: String, vars: HashMap<String, String> }
  InlinePrompt { prompt: String, agent: String }
```

**Config extension:** `defaultAgent` field on `.pu/config.yaml` — sets default agent type for spawning. Config also includes `agents` map (with per-agent `launchArgs` overrides) and `envFiles` list for environment file copying to worktrees.

```
Config:
  default_agent: String (default: "claude")
  agents: IndexMap<String, AgentConfig>
  env_files: Vec<String> (default: [".env", ".env.local"])

AgentConfig:
  name: String
  command: String
  prompt_flag: Option<String>
  interactive: bool (default: true)
  launch_args: Option<Vec<String>>
```

## Open Questions

? [DM-001] Should sessions be explicit user-created boundaries, or implicit based on time gaps in activity?

? [DM-002] How should the data model handle agent re-use across worktrees — new agent entry per worktree, or a single entry that moves?

? [DM-003] What agent types should be supported, and should the set be extensible?
(Current implementation: configurable via `.pu/config.yaml` with `AgentConfig` struct — `name`, `command`, `prompt_flag`, `interactive` flag. Default agent is "claude" with `command: "claude"`. The set is extensible via config.)

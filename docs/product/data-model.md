# Data Model

**Maturity: SEED** | ID Prefix: DM | Dependencies: none (defines types used by all other domains)

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

## Open Questions

? [DM-001] Should sessions be explicit user-created boundaries, or implicit based on time gaps in activity?

? [DM-002] How should the data model handle agent re-use across worktrees — new agent entry per worktree, or a single entry that moves?

? [DM-003] What agent types should be supported, and should the set be extensible?

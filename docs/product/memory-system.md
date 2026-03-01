# Memory System

**Maturity: SEED** | ID Prefix: MEM | Dependencies: none

## Purpose

Hierarchical memory for agents: per-agent, per-worktree, per-session, and per-project knowledge. Agents accumulate context that persists across restarts and sessions.

## Conceptual Model

The daemon acts as the context assembler (the "librarian"). The memory system isn't just per-agent state — it's the daemon's ability to assemble the right knowledge for each task. Two layers:

**Spec Library** — Design docs, architecture decisions, process rules. Lives in `docs/` as markdown files.

**Runtime Memory** — Per-agent, per-worktree, per-session state that accumulates during execution.

```
Context assembly flow:
  1. Task arrives
  2. Daemon classifies task → identifies relevant domains
  3. Daemon pulls specs + runtime memory
  4. Daemon formats context for the specific agent type
  5. Agent receives assembled context — never navigates for it

Memory scopes:
  project  — cross-session knowledge (patterns, conventions)
  session  — decisions and outcomes within a work session
  worktree — context for work in a specific worktree
  agent    — individual agent's accumulated context
```

## Open Questions

? [MEM-001] How should the daemon decide what runtime memory to include?
Per-agent memory grows over time. Should the daemon include all memory for a scope, use recency-based selection, or let agents request specific memory keys? What about memory size limits to avoid overwhelming agent context windows?

! [MEM-002] Agents write directly to spec files during single-agent execution; in parallel execution, agents report findings to a conductor who writes to specs — direct file editing for single-agent work (already established in spec-system.md Agent Writing Protocol), conductor-mediated writes for parallel execution (see Agent Communication Protocol in spec-system.md)

? [MEM-003] Where should runtime memory be stored?
Options: embedded database, flat files, external database, or something else entirely. Trade-offs around portability, query ability, and concurrent access need research.
